// ========================================
// ANALOS NFT LAUNCHPAD - SIMPLIFIED VERSION
// Ready for Solana Playground Deployment
// ========================================
//
// INSTRUCTIONS:
// 1. Go to https://beta.solpg.io
// 2. Create new Anchor project
// 3. Replace lib.rs with this file
// 4. Click "Build" to get your program ID
// 5. Update declare_id!() with your program ID
// 6. Build again
// 7. Connect wallet and set RPC to https://rpc.analos.io
// 8. Click "Deploy"!

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    keccak,
    program::invoke_signed,
    system_instruction,
};

declare_id!("11111111111111111111111111111111"); // Replace after first build!

/// Royalty basis points (500 = 5%)
pub const ROYALTY_BASIS_POINTS: u16 = 500;

#[program]
pub mod analos_nft_launchpad {
    use super::*;

    /// Initialize the collection
    ///
    /// Creates a new NFT collection with blind mint parameters
    pub fn initialize_collection(
        ctx: Context<InitializeCollection>,
        max_supply: u64,
        price_lamports: u64,
        reveal_threshold: u64,
        collection_name: String,
        collection_symbol: String,
        placeholder_uri: String,
    ) -> Result<()> {
        let config = &mut ctx.accounts.collection_config;
        
        config.authority = ctx.accounts.authority.key();
        config.max_supply = max_supply;
        config.price_lamports = price_lamports;
        config.reveal_threshold = reveal_threshold;
        config.current_supply = 0;
        config.is_revealed = false;
        config.is_paused = false;
        config.collection_name = collection_name;
        config.collection_symbol = collection_symbol;
        config.placeholder_uri = placeholder_uri;
        
        // Generate global seed for randomization
        let clock = Clock::get()?;
        let seed_data = [
            ctx.accounts.authority.key().as_ref(),
            &clock.unix_timestamp.to_le_bytes(),
            &clock.slot.to_le_bytes(),
        ].concat();
        let seed_hash = keccak::hash(&seed_data);
        config.global_seed = seed_hash.to_bytes();

        msg!("Collection initialized: {} - Max: {}, Price: {} lamports",
            config.collection_name, max_supply, price_lamports);

        Ok(())
    }

    /// Mint a placeholder NFT (mystery box)
    ///
    /// Users pay in LOS to receive a mystery box NFT
    pub fn mint_placeholder(
        ctx: Context<MintPlaceholder>,
    ) -> Result<()> {
        let config = &mut ctx.accounts.collection_config;

        // Validations
        require!(!config.is_paused, ErrorCode::CollectionPaused);
        require!(config.current_supply < config.max_supply, ErrorCode::SoldOut);

        let mint_index = config.current_supply;

        // Transfer payment from user to config PDA
        let transfer_ix = system_instruction::transfer(
            ctx.accounts.payer.key,
            &config.key(),
            config.price_lamports,
        );
        invoke_signed(
            &transfer_ix,
            &[
                ctx.accounts.payer.to_account_info(),
                config.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[],
        )?;

        // Generate pseudo-random rarity score for this NFT
        let rng_seed = [
            &config.global_seed[..],
            &mint_index.to_le_bytes(),
        ].concat();
        let trait_hash = keccak::hash(&rng_seed);
        let rarity_score = u64::from_le_bytes(trait_hash.to_bytes()[0..8].try_into().unwrap()) % 100;

        // Store mint record
        let mint_record = &mut ctx.accounts.mint_record;
        mint_record.mint_index = mint_index;
        mint_record.minter = ctx.accounts.payer.key();
        mint_record.is_revealed = false;
        mint_record.rarity_score = rarity_score;

        config.current_supply += 1;

        emit!(MintEvent {
            mint_index,
            minter: ctx.accounts.payer.key(),
            rarity_score,
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Minted NFT #{} for {} - Rarity score: {}", 
            mint_index, ctx.accounts.payer.key(), rarity_score);

        Ok(())
    }

    /// Reveal the collection
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
        config.placeholder_uri = revealed_base_uri.clone();

        emit!(RevealEvent {
            timestamp: Clock::get()?.unix_timestamp,
            total_minted: config.current_supply,
            revealed_base_uri,
        });

        msg!("Collection revealed! Total NFTs: {}", config.current_supply);

        Ok(())
    }

    /// Mark individual NFT as revealed
    pub fn reveal_nft(
        ctx: Context<RevealNft>,
    ) -> Result<()> {
        let config = &ctx.accounts.collection_config;
        require!(config.is_revealed, ErrorCode::NotRevealed);

        let mint_record = &mut ctx.accounts.mint_record;
        require!(!mint_record.is_revealed, ErrorCode::AlreadyRevealed);

        mint_record.is_revealed = true;

        // Determine rarity tier
        let rarity_tier = match mint_record.rarity_score {
            0..=4 => "Legendary",    // 5%
            5..=19 => "Epic",         // 15%
            20..=49 => "Rare",        // 30%
            _ => "Common",            // 50%
        };

        emit!(NftRevealedEvent {
            mint_index: mint_record.mint_index,
            rarity_tier: rarity_tier.to_string(),
            rarity_score: mint_record.rarity_score,
        });

        msg!("NFT #{} revealed: {} (score: {})", 
            mint_record.mint_index, rarity_tier, mint_record.rarity_score);

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

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction()]
pub struct MintPlaceholder<'info> {
    #[account(
        mut,
        seeds = [b"collection", collection_config.authority.as_ref()],
        bump,
    )]
    pub collection_config: Account<'info, CollectionConfig>,

    #[account(
        init,
        payer = payer,
        space = 8 + MintRecord::INIT_SPACE,
        seeds = [
            b"mint",
            collection_config.key().as_ref(),
            collection_config.current_supply.to_le_bytes().as_ref()
        ],
        bump
    )]
    pub mint_record: Account<'info, MintRecord>,

    #[account(mut)]
    pub payer: Signer<'info>,

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
pub struct RevealNft<'info> {
    #[account(
        seeds = [b"collection", collection_config.authority.as_ref()],
        bump,
    )]
    pub collection_config: Account<'info, CollectionConfig>,

    #[account(
        mut,
        seeds = [
            b"mint",
            collection_config.key().as_ref(),
            mint_record.mint_index.to_le_bytes().as_ref()
        ],
        bump
    )]
    pub mint_record: Account<'info, MintRecord>,
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
    pub authority: Pubkey,
    pub max_supply: u64,
    pub current_supply: u64,
    pub price_lamports: u64,
    pub reveal_threshold: u64,
    pub is_revealed: bool,
    pub is_paused: bool,
    pub global_seed: [u8; 32],
    #[max_len(32)]
    pub collection_name: String,
    #[max_len(10)]
    pub collection_symbol: String,
    #[max_len(200)]
    pub placeholder_uri: String,
}

#[account]
#[derive(InitSpace)]
pub struct MintRecord {
    pub mint_index: u64,
    pub minter: Pubkey,
    pub is_revealed: bool,
    pub rarity_score: u64,
}

// ========== EVENTS ==========

#[event]
pub struct MintEvent {
    pub mint_index: u64,
    pub minter: Pubkey,
    pub rarity_score: u64,
    pub timestamp: i64,
}

#[event]
pub struct RevealEvent {
    pub timestamp: i64,
    pub total_minted: u64,
    pub revealed_base_uri: String,
}

#[event]
pub struct NftRevealedEvent {
    pub mint_index: u64,
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
    #[msg("Insufficient funds for withdrawal")]
    InsufficientFunds,
    #[msg("Invalid threshold value")]
    InvalidThreshold,
}

