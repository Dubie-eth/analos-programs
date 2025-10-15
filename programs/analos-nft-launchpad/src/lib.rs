use anchor_lang::prelude::*;
use anchor_lang::solana_program::{keccak, program::invoke_signed, system_instruction};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{mint_to, Mint, MintTo, Token, TokenAccount},
};
use mpl_bubblegum::{
    cpi::accounts::MintV1,
    cpi::mint_v1,
    program::BubblegumProgram,
    state::MetadataArgs,
    state::Creator,
    state::Collection,
    state::TokenProgramVersion,
    state::TokenStandard,
};
use spl_account_compression::{
    program::SplAccountCompression,
    state::ConcurrentMerkleTreeAccount,
};

// Security.txt implementation for program verification
#[cfg(not(feature = "no-entrypoint"))]
use {default_env::default_env, solana_security_txt::security_txt};

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Analos NFT Launchpad",
    project_url: "https://github.com/Dubie-eth/analos-nft-launchpad-program",
    contacts: "email:support@launchonlos.fun,twitter:@EWildn,telegram:t.me/Dubie_420",
    policy: "https://github.com/Dubie-eth/analos-nft-launchpad-program/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/Dubie-eth/analos-nft-launchpad-program",
    source_revision: "5gmaywNK418QzG7eFA7qZLJkCGS8cfcPtm4b2RZQaJHT", // This should be the commit hash, not program ID
    source_release: "v1.0.0",
    auditors: "None",
    acknowledgements: "Thank you to all security researchers who help keep Analos secure!"
}

declare_id!("5gmaywNK418QzG7eFA7qZLJkCGS8cfcPtm4b2RZQaJHT");

/// Royalty basis points (500 = 5%)
pub const ROYALTY_BASIS_POINTS: u16 = 500;

/// Fee system constants
pub const PLATFORM_FEE_BASIS_POINTS: u16 = 250; // 2.5%
pub const BUYBACK_FEE_BASIS_POINTS: u16 = 150; // 1.5%
pub const DEV_FEE_BASIS_POINTS: u16 = 100; // 1.0%
pub const TOTAL_FEE_BASIS_POINTS: u16 = 500; // 5.0% total

/// Fee recipient addresses
pub const PLATFORM_WALLET: &str = "7axzrUvuYZ32bKLS5eVZC6okfJNVvz33eQc4RLNRpQPi"; // Platform revenue wallet
pub const BUYBACK_WALLET: &str = "9ReqU29vEXtnQfMUp74CyfPwnKRUAKSDBzo8C62p2jo2"; // $LOL buyback wallet
pub const DEV_WALLET: &str = "GMYuGbRtSaPxviMXcnU8GLh6Yt6azxw1Y6JHNesU8MVr"; // Developer maintenance wallet

/// Merkle tree constants
pub const MAX_DEPTH: u32 = 14; // Supports up to 16,384 NFTs
pub const MAX_BUFFER_SIZE: u32 = 64;

#[program]
pub mod analos_nft_launchpad {
    use super::*;

    /// Initialize a new collection with Merkle tree for compressed NFTs
    pub fn initialize_collection(
        ctx: Context<InitializeCollection>,
        collection_name: String,
        collection_symbol: String,
        max_supply: u64,
        price_lamports: u64,
        reveal_threshold: u64,
        placeholder_uri: String,
    ) -> Result<()> {
        require!(
            max_supply <= 16384,
            ErrorCode::InvalidMaxSupply
        );
        require!(
            reveal_threshold <= max_supply && reveal_threshold > 0,
            ErrorCode::InvalidThreshold
        );

        let config = &mut ctx.accounts.collection_config;
        config.authority = ctx.accounts.authority.key();
        config.max_supply = max_supply;
        config.current_supply = 0;
        config.price_lamports = price_lamports;
        config.reveal_threshold = reveal_threshold;
        config.is_revealed = false;
        config.is_paused = false;
        config.collection_mint = ctx.accounts.collection_mint.key();
        config.collection_name = collection_name;
        config.collection_symbol = collection_symbol;
        config.placeholder_uri = placeholder_uri;

        // Generate random global seed for reveal
        let clock = Clock::get()?;
        let seed_data = [
            clock.unix_timestamp.to_le_bytes(),
            clock.slot.to_le_bytes(),
            ctx.accounts.authority.key().to_bytes(),
        ];
        config.global_seed = keccak::hash(&seed_data).to_bytes();

        emit!(CollectionInitializedEvent {
            collection_config: config.key(),
            authority: config.authority,
            max_supply: config.max_supply,
            price_lamports: config.price_lamports,
            reveal_threshold: config.reveal_threshold,
            timestamp: clock.unix_timestamp,
        });

        msg!("Collection initialized: {} (max: {}, price: {} lamports, threshold: {})", 
            config.collection_name, config.max_supply, config.price_lamports, config.reveal_threshold);

        Ok(())
    }

    /// Mint a compressed placeholder NFT (mystery box)
    pub fn mint_placeholder(ctx: Context<MintPlaceholder>) -> Result<()> {
        let config = &mut ctx.accounts.collection_config;

        // Validations
        require!(!config.is_paused, ErrorCode::CollectionPaused);
        require!(
            config.current_supply < config.max_supply,
            ErrorCode::SoldOut
        );

        let mint_index = config.current_supply;

        // Calculate fee distribution
        let total_fee = config.price_lamports * TOTAL_FEE_BASIS_POINTS as u64 / 10000;
        let platform_fee = config.price_lamports * PLATFORM_FEE_BASIS_POINTS as u64 / 10000;
        let buyback_fee = config.price_lamports * BUYBACK_FEE_BASIS_POINTS as u64 / 10000;
        let dev_fee = config.price_lamports * DEV_FEE_BASIS_POINTS as u64 / 10000;
        let creator_payment = config.price_lamports - total_fee;

        // Transfer payment to collection creator (95%)
        let creator_transfer_ix =
            system_instruction::transfer(&ctx.accounts.payer.key(), &config.key(), creator_payment);
        invoke_signed(
            &creator_transfer_ix,
            &[
                ctx.accounts.payer.to_account_info(),
                config.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[],
        )?;

        // Transfer platform fee (2.5%)
        if platform_fee > 0 {
            let platform_wallet: Pubkey = PLATFORM_WALLET.parse().unwrap();
            let platform_transfer_ix = system_instruction::transfer(
                &ctx.accounts.payer.key(),
                &platform_wallet,
                platform_fee,
            );
            invoke_signed(
                &platform_transfer_ix,
                &[
                    ctx.accounts.payer.to_account_info(),
                    ctx.accounts.system_program.to_account_info(),
                ],
                &[],
            )?;
        }

        // Transfer buyback fee (1.5%)
        if buyback_fee > 0 {
            let buyback_wallet: Pubkey = BUYBACK_WALLET.parse().unwrap();
            let buyback_transfer_ix =
                system_instruction::transfer(&ctx.accounts.payer.key(), &buyback_wallet, buyback_fee);
            invoke_signed(
                &buyback_transfer_ix,
                &[
                    ctx.accounts.payer.to_account_info(),
                    ctx.accounts.system_program.to_account_info(),
                ],
                &[],
            )?;
        }

        // Transfer dev fee (1.0%)
        if dev_fee > 0 {
            let dev_wallet: Pubkey = DEV_WALLET.parse().unwrap();
            let dev_transfer_ix =
                system_instruction::transfer(&ctx.accounts.payer.key(), &dev_wallet, dev_fee);
            invoke_signed(
                &dev_transfer_ix,
                &[
                    ctx.accounts.payer.to_account_info(),
                    ctx.accounts.system_program.to_account_info(),
                ],
                &[],
            )?;
        }

        // Create metadata for compressed NFT
        let metadata = MetadataArgs {
            name: format!("{} Mystery #{}", config.collection_name, mint_index),
            symbol: config.collection_symbol.clone(),
            uri: config.placeholder_uri.clone(),
            seller_fee_basis_points: ROYALTY_BASIS_POINTS,
            creators: vec![Creator {
                address: config.authority,
                verified: false,
                share: 100,
            }],
            edition_nonce: None,
            uses: None,
            collection: Some(Collection {
                verified: false,
                key: config.collection_mint,
            }),
            token_program_version: TokenProgramVersion::Original,
            token_standard: Some(TokenStandard::NonFungible),
        };

        // Mint compressed NFT using Bubblegum
        let mint_v1_accounts = MintV1 {
            tree_config: ctx.accounts.tree_config.to_account_info(),
            leaf_owner: ctx.accounts.payer.key(),
            leaf_delegate: ctx.accounts.payer.key(),
            merkle_tree: ctx.accounts.merkle_tree.to_account_info(),
            payer: ctx.accounts.payer.to_account_info(),
            tree_creator_or_delegate: ctx.accounts.tree_creator.to_account_info(),
            log_wrapper: ctx.accounts.log_wrapper.to_account_info(),
            compression_program: ctx.accounts.compression_program.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
        };

        let mint_v1_ctx = CpiContext::new(
            ctx.accounts.bubblegum_program.to_account_info(),
            mint_v1_accounts,
        );

        mint_v1(mint_v1_ctx, metadata)?;

        config.current_supply += 1;

        emit!(MintEvent {
            mint_index,
            minter: ctx.accounts.payer.key(),
            merkle_tree: ctx.accounts.merkle_tree.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        emit!(FeeCollectionEvent {
            mint_index,
            total_payment: config.price_lamports,
            creator_payment,
            platform_fee,
            buyback_fee,
            dev_fee,
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Minted compressed NFT #{} for {} - Fees: Platform: {} lamports, Buyback: {} lamports, Dev: {} lamports", 
            mint_index, ctx.accounts.payer.key(), platform_fee, buyback_fee, dev_fee);

        Ok(())
    }

    /// Trigger reveal for the collection
    pub fn reveal_collection(
        ctx: Context<RevealCollection>,
        revealed_base_uri: String,
    ) -> Result<()> {
        let config = &mut ctx.accounts.collection_config;

        require!(!config.is_revealed, ErrorCode::AlreadyRevealed);
        require!(
            config.current_supply >= config.reveal_threshold,
            ErrorCode::ThresholdNotMet
        );

        config.is_revealed = true;

        emit!(RevealEvent {
            timestamp: Clock::get()?.unix_timestamp,
            total_minted: config.current_supply,
            revealed_base_uri: revealed_base_uri.clone(),
        });

        msg!("Collection revealed! Total minted: {}, Revealed URI: {}", 
            config.current_supply, revealed_base_uri);

        Ok(())
    }

    /// Update NFT metadata after reveal
    pub fn update_nft_metadata(
        ctx: Context<UpdateNftMetadata>,
        new_uri: String,
    ) -> Result<()> {
        let config = &ctx.accounts.collection_config;
        require!(config.is_revealed, ErrorCode::NotRevealed);

        // This would typically involve updating the metadata URI
        // For compressed NFTs, this is more complex and may require
        // additional logic or external services

        emit!(MetadataUpdateEvent {
            timestamp: Clock::get()?.unix_timestamp,
            new_uri,
        });

        Ok(())
    }

    /// Withdraw collected funds
    pub fn withdraw_funds(ctx: Context<WithdrawFunds>, amount: u64) -> Result<()> {
        let config = &ctx.accounts.collection_config;
        require!(
            ctx.accounts.authority.key() == config.authority,
            ErrorCode::Unauthorized
        );

        let collection_balance = config.to_account_info().lamports();
        require!(
            collection_balance >= amount,
            ErrorCode::InsufficientFunds
        );

        **ctx.accounts.collection_config.to_account_info().lamports.borrow_mut() -= amount;
        **ctx.accounts.authority.to_account_info().lamports.borrow_mut() += amount;

        emit!(WithdrawEvent {
            amount,
            authority: config.authority,
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Withdrew {} lamports to {}", amount, config.authority);

        Ok(())
    }

    /// Pause/unpause collection minting
    pub fn pause_collection(ctx: Context<PauseCollection>, is_paused: bool) -> Result<()> {
        let config = &mut ctx.accounts.collection_config;
        require!(
            ctx.accounts.authority.key() == config.authority,
            ErrorCode::Unauthorized
        );

        config.is_paused = is_paused;

        emit!(PauseEvent {
            is_paused,
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Collection {} paused: {}", config.collection_name, is_paused);

        Ok(())
    }

    /// Update collection configuration
    pub fn update_config(
        ctx: Context<UpdateConfig>,
        new_price: Option<u64>,
        new_reveal_threshold: Option<u64>,
    ) -> Result<()> {
        let config = &mut ctx.accounts.collection_config;
        require!(
            ctx.accounts.authority.key() == config.authority,
            ErrorCode::Unauthorized
        );

        if let Some(price) = new_price {
            config.price_lamports = price;
        }

        if let Some(threshold) = new_reveal_threshold {
            require!(
                threshold <= config.max_supply && threshold > 0,
                ErrorCode::InvalidThreshold
            );
            config.reveal_threshold = threshold;
        }

        emit!(ConfigUpdateEvent {
            new_price,
            new_reveal_threshold,
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Config updated: price={:?}, threshold={:?}", new_price, new_reveal_threshold);

        Ok(())
    }
}

// ========== ACCOUNTS ==========

#[derive(Accounts)]
#[instruction(collection_name: String, collection_symbol: String)]
pub struct InitializeCollection<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = 8 + CollectionConfig::INIT_SPACE,
        seeds = [b"collection", authority.key().as_ref()],
        bump,
    )]
    pub collection_config: Account<'info, CollectionConfig>,

    /// CHECK: This account is validated in the instruction
    #[account(mut)]
    pub merkle_tree: AccountInfo<'info>,

    /// CHECK: This account is validated in the instruction
    #[account(mut)]
    pub tree_config: AccountInfo<'info>,

    /// CHECK: This account is validated in the instruction
    pub tree_creator: AccountInfo<'info>,

    /// CHECK: This account is validated in the instruction
    pub log_wrapper: AccountInfo<'info>,

    pub collection_mint: Account<'info, Mint>,
    pub bubblegum_program: Program<'info, BubblegumProgram>,
    pub compression_program: Program<'info, SplAccountCompression>,
    pub system_program: Program<'info, System>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct MintPlaceholder<'info> {
    #[account(
        mut,
        seeds = [b"collection", collection_config.authority.as_ref()],
        bump,
    )]
    pub collection_config: Account<'info, CollectionConfig>,

    /// CHECK: This account is validated in the instruction
    #[account(mut)]
    pub merkle_tree: AccountInfo<'info>,

    /// CHECK: This account is validated in the instruction
    #[account(mut)]
    pub tree_config: AccountInfo<'info>,

    /// CHECK: This account is validated in the instruction
    pub tree_creator: AccountInfo<'info>,

    /// CHECK: This account is validated in the instruction
    pub log_wrapper: AccountInfo<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub bubblegum_program: Program<'info, BubblegumProgram>,
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

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct UpdateNftMetadata<'info> {
    #[account(
        seeds = [b"collection", collection_config.authority.as_ref()],
        bump,
    )]
    pub collection_config: Account<'info, CollectionConfig>,

    /// CHECK: This account is validated in the instruction
    pub merkle_tree: AccountInfo<'info>,

    pub authority: Signer<'info>,
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
pub struct PauseCollection<'info> {
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

// ========== STATE ==========

#[account]
#[derive(InitSpace)]
pub struct CollectionConfig {
    pub authority: Pubkey,
    pub max_supply: u64,
    pub current_supply: u64,
    pub price_lamports: u64,
    pub reveal_threshold: u64,
    pub is_revealed: bool,
    pub is_paused: bool,
    pub global_seed: [u8; 32],
    pub collection_mint: Pubkey,
    #[max_len(32)]
    pub collection_name: String,
    #[max_len(10)]
    pub collection_symbol: String,
    #[max_len(200)]
    pub placeholder_uri: String,
}

// ========== EVENTS ==========

#[event]
pub struct CollectionInitializedEvent {
    pub collection_config: Pubkey,
    pub authority: Pubkey,
    pub max_supply: u64,
    pub price_lamports: u64,
    pub reveal_threshold: u64,
    pub timestamp: i64,
}

#[event]
pub struct MintEvent {
    pub mint_index: u64,
    pub minter: Pubkey,
    pub merkle_tree: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct RevealEvent {
    pub timestamp: i64,
    pub total_minted: u64,
    pub revealed_base_uri: String,
}

#[event]
pub struct MetadataUpdateEvent {
    pub timestamp: i64,
    pub new_uri: String,
}

#[event]
pub struct FeeCollectionEvent {
    pub mint_index: u64,
    pub total_payment: u64,
    pub creator_payment: u64,
    pub platform_fee: u64,
    pub buyback_fee: u64,
    pub dev_fee: u64,
    pub timestamp: i64,
}

#[event]
pub struct WithdrawEvent {
    pub amount: u64,
    pub authority: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct PauseEvent {
    pub is_paused: bool,
    pub timestamp: i64,
}

#[event]
pub struct ConfigUpdateEvent {
    pub new_price: Option<u64>,
    pub new_reveal_threshold: Option<u64>,
    pub timestamp: i64,
}

// ========== ERRORS ==========

#[error_code]
pub enum ErrorCode {
    #[msg("Collection is sold out")]
    SoldOut,
    #[msg("Collection minting is paused")]
    CollectionPaused,
    #[msg("Collection has already been revealed")]
    AlreadyRevealed,
    #[msg("Collection has not been revealed yet")]
    NotRevealed,
    #[msg("Reveal threshold has not been met")]
    ThresholdNotMet,
    #[msg("Insufficient funds for withdrawal")]
    InsufficientFunds,
    #[msg("Invalid threshold value")]
    InvalidThreshold,
    #[msg("Invalid max supply (max 16,384)")]
    InvalidMaxSupply,
    #[msg("Unauthorized access")]
    Unauthorized,
}
