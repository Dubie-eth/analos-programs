use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    keccak,
    program::invoke,
    system_instruction,
};
use anchor_spl::token::{Mint, Token, TokenAccount};
use mpl_bubblegum::state::metaplex_adapter::MetadataArgs;
use mpl_bubblegum::state::metaplex_anchor::{MplTokenMetadata};
use spl_account_compression::{program::SplAccountCompression, Noop};

declare_id!("FAS9AgPy9SbyBeHCyiF5YBUYt7HbAwRF5Kie9CzBXtwJ");

/// Maximum depth for the Merkle tree (supports 2^14 = 16,384 NFTs)
pub const MAX_DEPTH: usize = 14;
/// Maximum buffer size for concurrent Merkle updates
pub const MAX_BUFFER_SIZE: usize = 64;
/// Basis points for royalties (500 = 5%)
pub const ROYALTY_BASIS_POINTS: u16 = 500;

#[program]
pub mod analos_nft_launchpad {
    use super::*;

    /// Initialize the collection with a new Merkle tree for compressed NFTs
    /// 
    /// # Arguments
    /// * `max_supply` - Maximum number of NFTs that can be minted
    /// * `price_lamports` - Price per mint in lamports (1 LOS = 1e9 lamports)
    /// * `reveal_threshold` - Number of mints required before reveal can trigger
    /// * `collection_name` - Name of the collection
    /// * `collection_symbol` - Symbol for the collection
    /// * `collection_uri` - URI for collection metadata
    pub fn initialize_collection(
        ctx: Context<InitializeCollection>,
        max_supply: u64,
        price_lamports: u64,
        reveal_threshold: u64,
        collection_name: String,
        collection_symbol: String,
        collection_uri: String,
    ) -> Result<()> {
        let config = &mut ctx.accounts.collection_config;
        
        // Initialize collection configuration
        config.authority = ctx.accounts.authority.key();
        config.max_supply = max_supply;
        config.price_lamports = price_lamports;
        config.reveal_threshold = reveal_threshold;
        config.current_supply = 0;
        config.is_revealed = false;
        config.is_paused = false;
        config.merkle_tree = ctx.accounts.merkle_tree.key();
        config.tree_authority = ctx.accounts.tree_authority.key();
        config.collection_mint = ctx.accounts.collection_mint.key();
        config.collection_name = collection_name;
        config.collection_symbol = collection_symbol;
        config.collection_uri = collection_uri;
        
        // Generate global seed for randomization
        let clock = Clock::get()?;
        let seed_data = [
            ctx.accounts.authority.key().as_ref(),
            &clock.unix_timestamp.to_le_bytes(),
            &clock.slot.to_le_bytes(),
        ].concat();
        let seed_hash = keccak::hash(&seed_data);
        config.global_seed = seed_hash.to_bytes();

        msg!("Collection initialized: {} ({})", config.collection_name, config.collection_symbol);
        msg!("Max supply: {}, Price: {} lamports", max_supply, price_lamports);
        msg!("Merkle tree: {}", config.merkle_tree);

        Ok(())
    }

    /// Mint a placeholder compressed NFT (mystery box)
    /// 
    /// Users pay in LOS to receive an unrevealed cNFT with placeholder metadata.
    /// The cNFT can be traded before reveal.
    pub fn mint_placeholder(
        ctx: Context<MintPlaceholder>,
    ) -> Result<()> {
        let config = &mut ctx.accounts.collection_config;

        // Validation checks
        require!(!config.is_paused, ErrorCode::CollectionPaused);
        require!(config.current_supply < config.max_supply, ErrorCode::SoldOut);

        let mint_index = config.current_supply;

        // Transfer payment from user to collection config PDA
        let transfer_ix = system_instruction::transfer(
            ctx.accounts.payer.key,
            &config.key(),
            config.price_lamports,
        );
        invoke(
            &transfer_ix,
            &[
                ctx.accounts.payer.to_account_info(),
                config.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        // Prepare placeholder metadata
        let metadata_args = MetadataArgs {
            name: format!("Analos Mystery #{}", mint_index),
            symbol: config.collection_symbol.clone(),
            uri: config.collection_uri.clone(), // Points to placeholder JSON
            seller_fee_basis_points: ROYALTY_BASIS_POINTS,
            primary_sale_happened: false,
            is_mutable: true, // Must be mutable for reveal
            edition_nonce: Some(0),
            token_standard: Some(mpl_bubblegum::state::TokenStandard::NonFungible),
            collection: Some(mpl_bubblegum::state::Collection {
                verified: false,
                key: config.collection_mint,
            }),
            uses: None,
            token_program_version: mpl_bubblegum::state::TokenProgramVersion::Original,
            creators: vec![
                mpl_bubblegum::state::Creator {
                    address: config.authority,
                    verified: false,
                    share: 100,
                }
            ],
        };

        // CPI to Bubblegum to mint the compressed NFT
        let seeds = &[
            b"collection".as_ref(),
            config.authority.as_ref(),
            &[ctx.bumps.collection_config],
        ];
        let signer_seeds = &[&seeds[..]];

        let cpi_accounts = mpl_bubblegum::cpi::accounts::MintV1 {
            tree_authority: ctx.accounts.tree_authority.to_account_info(),
            leaf_owner: ctx.accounts.payer.to_account_info(),
            leaf_delegate: ctx.accounts.payer.to_account_info(),
            merkle_tree: ctx.accounts.merkle_tree.to_account_info(),
            payer: ctx.accounts.payer.to_account_info(),
            tree_delegate: ctx.accounts.collection_config.to_account_info(),
            log_wrapper: ctx.accounts.log_wrapper.to_account_info(),
            compression_program: ctx.accounts.compression_program.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
        };

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.bubblegum_program.to_account_info(),
            cpi_accounts,
            signer_seeds,
        );

        mpl_bubblegum::cpi::mint_v1(cpi_ctx, metadata_args)?;

        // Update supply counter
        config.current_supply += 1;

        emit!(MintEvent {
            mint_index,
            minter: ctx.accounts.payer.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Minted placeholder NFT #{} for {}", mint_index, ctx.accounts.payer.key());

        Ok(())
    }

    /// Reveal collection by updating metadata with randomized traits
    /// 
    /// Can be triggered by admin after threshold is met or manually.
    /// Uses pseudo-RNG to assign traits to each NFT.
    pub fn reveal_collection(
        ctx: Context<RevealCollection>,
        revealed_base_uri: String,
    ) -> Result<()> {
        let config = &mut ctx.accounts.collection_config;

        // Validation
        require!(!config.is_revealed, ErrorCode::AlreadyRevealed);
        require!(
            config.current_supply >= config.reveal_threshold,
            ErrorCode::ThresholdNotMet
        );

        config.is_revealed = true;
        config.collection_uri = revealed_base_uri.clone();

        emit!(RevealEvent {
            timestamp: Clock::get()?.unix_timestamp,
            total_minted: config.current_supply,
            revealed_base_uri: revealed_base_uri.clone(),
        });

        msg!("Collection revealed! Total NFTs: {}", config.current_supply);
        msg!("New base URI: {}", revealed_base_uri);

        Ok(())
    }

    /// Update metadata for individual NFT post-reveal
    /// 
    /// Called per-NFT to assign randomized traits and update URI.
    /// Must be called after reveal_collection.
    pub fn update_nft_metadata(
        ctx: Context<UpdateNftMetadata>,
        mint_index: u64,
        root: [u8; 32],
        data_hash: [u8; 32],
        creator_hash: [u8; 32],
        nonce: u64,
        index: u32,
    ) -> Result<()> {
        let config = &ctx.accounts.collection_config;

        require!(config.is_revealed, ErrorCode::NotRevealed);
        require!(mint_index < config.current_supply, ErrorCode::InvalidMintIndex);

        // Generate pseudo-random trait assignment
        let rng_seed = [
            &config.global_seed[..],
            &mint_index.to_le_bytes(),
        ].concat();
        let trait_hash = keccak::hash(&rng_seed);
        let rarity_score = u64::from_le_bytes(trait_hash.to_bytes()[0..8].try_into().unwrap()) % 100;

        // Determine rarity tier based on score
        let rarity_tier = match rarity_score {
            0..=4 => "Legendary",    // 5%
            5..=19 => "Epic",         // 15%
            20..=49 => "Rare",        // 30%
            _ => "Common",            // 50%
        };

        // Update metadata with new URI pointing to revealed traits
        let new_uri = format!("{}{}.json", config.collection_uri, mint_index);
        
        let new_metadata = MetadataArgs {
            name: format!("{} #{}", config.collection_name, mint_index),
            symbol: config.collection_symbol.clone(),
            uri: new_uri.clone(),
            seller_fee_basis_points: ROYALTY_BASIS_POINTS,
            primary_sale_happened: false,
            is_mutable: true,
            edition_nonce: Some(0),
            token_standard: Some(mpl_bubblegum::state::TokenStandard::NonFungible),
            collection: Some(mpl_bubblegum::state::Collection {
                verified: false,
                key: config.collection_mint,
            }),
            uses: None,
            token_program_version: mpl_bubblegum::state::TokenProgramVersion::Original,
            creators: vec![
                mpl_bubblegum::state::Creator {
                    address: config.authority,
                    verified: false,
                    share: 100,
                }
            ],
        };

        // CPI to update the leaf in the Merkle tree
        let seeds = &[
            b"collection".as_ref(),
            config.authority.as_ref(),
            &[ctx.bumps.collection_config],
        ];
        let signer_seeds = &[&seeds[..]];

        let cpi_accounts = mpl_bubblegum::cpi::accounts::UpdateMetadata {
            tree_authority: ctx.accounts.tree_authority.to_account_info(),
            authority: ctx.accounts.collection_config.to_account_info(),
            merkle_tree: ctx.accounts.merkle_tree.to_account_info(),
            payer: ctx.accounts.authority.to_account_info(),
            leaf_owner: ctx.accounts.leaf_owner.to_account_info(),
            leaf_delegate: ctx.accounts.leaf_owner.to_account_info(),
            log_wrapper: ctx.accounts.log_wrapper.to_account_info(),
            compression_program: ctx.accounts.compression_program.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
        };

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.bubblegum_program.to_account_info(),
            cpi_accounts,
            signer_seeds,
        );

        mpl_bubblegum::cpi::update_metadata(
            cpi_ctx,
            root,
            data_hash,
            creator_hash,
            nonce,
            index,
            new_metadata,
        )?;

        emit!(UpdateMetadataEvent {
            mint_index,
            new_uri,
            rarity_tier: rarity_tier.to_string(),
            rarity_score,
        });

        msg!("Updated NFT #{}: {} - {}", mint_index, rarity_tier, new_uri);

        Ok(())
    }

    /// Withdraw collected funds (admin only)
    pub fn withdraw_funds(
        ctx: Context<WithdrawFunds>,
        amount: u64,
    ) -> Result<()> {
        let config = &ctx.accounts.collection_config;
        
        let config_lamports = config.to_account_info().lamports();
        let rent_exempt = Rent::get()?.minimum_balance(config.to_account_info().data_len());
        
        require!(
            config_lamports.checked_sub(amount).unwrap() >= rent_exempt,
            ErrorCode::InsufficientFunds
        );

        **config.to_account_info().try_borrow_mut_lamports()? -= amount;
        **ctx.accounts.authority.to_account_info().try_borrow_mut_lamports()? += amount;

        msg!("Withdrawn {} lamports to authority", amount);

        Ok(())
    }

    /// Pause/unpause minting (admin only)
    pub fn set_pause(
        ctx: Context<SetPause>,
        paused: bool,
    ) -> Result<()> {
        ctx.accounts.collection_config.is_paused = paused;
        msg!("Collection paused: {}", paused);
        Ok(())
    }

    /// Update collection parameters (admin only)
    pub fn update_config(
        ctx: Context<UpdateConfig>,
        new_price: Option<u64>,
        new_reveal_threshold: Option<u64>,
    ) -> Result<()> {
        let config = &mut ctx.accounts.collection_config;

        if let Some(price) = new_price {
            config.price_lamports = price;
            msg!("Updated price to {} lamports", price);
        }

        if let Some(threshold) = new_reveal_threshold {
            require!(threshold <= config.max_supply, ErrorCode::InvalidThreshold);
            config.reveal_threshold = threshold;
            msg!("Updated reveal threshold to {}", threshold);
        }

        Ok(())
    }
}

// ========== ACCOUNTS STRUCTS ==========

#[derive(Accounts)]
pub struct InitializeCollection<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + CollectionConfig::INIT_SPACE,
        seeds = [b"collection", authority.key().as_ref()],
        bump
    )]
    pub collection_config: Account<'info, CollectionConfig>,

    /// The Merkle tree account for storing compressed NFTs
    /// CHECK: Validated by Bubblegum program
    #[account(mut)]
    pub merkle_tree: UncheckedAccount<'info>,

    /// Tree authority PDA (controlled by Bubblegum)
    /// CHECK: Validated by Bubblegum program
    #[account(mut)]
    pub tree_authority: UncheckedAccount<'info>,

    /// Collection mint account
    #[account(
        init,
        payer = authority,
        mint::decimals = 0,
        mint::authority = collection_config,
        mint::freeze_authority = collection_config,
    )]
    pub collection_mint: Account<'info, Mint>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct MintPlaceholder<'info> {
    #[account(
        mut,
        seeds = [b"collection", collection_config.authority.as_ref()],
        bump,
    )]
    pub collection_config: Account<'info, CollectionConfig>,

    /// CHECK: Validated by Bubblegum program
    #[account(mut)]
    pub merkle_tree: UncheckedAccount<'info>,

    /// CHECK: Validated by Bubblegum program
    #[account(mut)]
    pub tree_authority: UncheckedAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: Bubblegum program
    #[account(address = mpl_bubblegum::ID)]
    pub bubblegum_program: UncheckedAccount<'info>,

    pub log_wrapper: Program<'info, Noop>,
    pub compression_program: Program<'info, SplAccountCompression>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RevealCollection<'info> {
    #[account(
        mut,
        seeds = [b"collection", authority.key().as_ref()],
        bump,
        has_one = authority,
    )]
    pub collection_config: Account<'info, CollectionConfig>,

    #[account(mut)]
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct UpdateNftMetadata<'info> {
    #[account(
        mut,
        seeds = [b"collection", collection_config.authority.as_ref()],
        bump,
    )]
    pub collection_config: Account<'info, CollectionConfig>,

    /// CHECK: Validated by Bubblegum program
    #[account(mut)]
    pub merkle_tree: UncheckedAccount<'info>,

    /// CHECK: Validated by Bubblegum program
    #[account(mut)]
    pub tree_authority: UncheckedAccount<'info>,

    /// CHECK: Current owner of the NFT leaf
    pub leaf_owner: UncheckedAccount<'info>,

    #[account(mut, address = collection_config.authority)]
    pub authority: Signer<'info>,

    /// CHECK: Bubblegum program
    #[account(address = mpl_bubblegum::ID)]
    pub bubblegum_program: UncheckedAccount<'info>,

    pub log_wrapper: Program<'info, Noop>,
    pub compression_program: Program<'info, SplAccountCompression>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WithdrawFunds<'info> {
    #[account(
        mut,
        seeds = [b"collection", authority.key().as_ref()],
        bump,
        has_one = authority,
    )]
    pub collection_config: Account<'info, CollectionConfig>,

    #[account(mut)]
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetPause<'info> {
    #[account(
        mut,
        seeds = [b"collection", authority.key().as_ref()],
        bump,
        has_one = authority,
    )]
    pub collection_config: Account<'info, CollectionConfig>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(
        mut,
        seeds = [b"collection", authority.key().as_ref()],
        bump,
        has_one = authority,
    )]
    pub collection_config: Account<'info, CollectionConfig>,

    pub authority: Signer<'info>,
}

// ========== STATE STRUCTS ==========

#[account]
#[derive(InitSpace)]
pub struct CollectionConfig {
    /// Authority that can manage the collection
    pub authority: Pubkey,
    /// Maximum number of NFTs in the collection
    pub max_supply: u64,
    /// Current number of minted NFTs
    pub current_supply: u64,
    /// Price per mint in lamports
    pub price_lamports: u64,
    /// Number of mints required before reveal
    pub reveal_threshold: u64,
    /// Whether the collection has been revealed
    pub is_revealed: bool,
    /// Whether minting is paused
    pub is_paused: bool,
    /// Global seed for randomization
    pub global_seed: [u8; 32],
    /// Merkle tree account storing compressed NFTs
    pub merkle_tree: Pubkey,
    /// Tree authority PDA
    pub tree_authority: Pubkey,
    /// Collection mint
    pub collection_mint: Pubkey,
    /// Collection name
    #[max_len(32)]
    pub collection_name: String,
    /// Collection symbol
    #[max_len(10)]
    pub collection_symbol: String,
    /// Base URI for metadata (placeholder before reveal)
    #[max_len(200)]
    pub collection_uri: String,
}

// ========== EVENTS ==========

#[event]
pub struct MintEvent {
    pub mint_index: u64,
    pub minter: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct RevealEvent {
    pub timestamp: i64,
    pub total_minted: u64,
    pub revealed_base_uri: String,
}

#[event]
pub struct UpdateMetadataEvent {
    pub mint_index: u64,
    pub new_uri: String,
    pub rarity_tier: String,
    pub rarity_score: u64,
}

// ========== ERROR CODES ==========

#[error_code]
pub enum ErrorCode {
    #[msg("Collection is sold out")]
    SoldOut,
    #[msg("Collection minting is paused")]
    CollectionPaused,
    #[msg("Collection has already been revealed")]
    AlreadyRevealed,
    #[msg("Reveal threshold has not been met")]
    ThresholdNotMet,
    #[msg("Collection has not been revealed yet")]
    NotRevealed,
    #[msg("Invalid mint index")]
    InvalidMintIndex,
    #[msg("Insufficient funds for withdrawal")]
    InsufficientFunds,
    #[msg("Invalid threshold value")]
    InvalidThreshold,
}
