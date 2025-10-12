use anchor_lang::prelude::*;
use anchor_spl::{
    token::{self, Mint, Token, TokenAccount, MintTo, Burn, Transfer},
    associated_token::AssociatedToken,
};

// Security.txt implementation for program verification
#[cfg(not(feature = "no-entrypoint"))]
use {default_env::default_env, solana_security_txt::security_txt};

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Analos Token Launch",
    project_url: "https://github.com/Dubie-eth/analos-programs",
    contacts: "email:security@analos.io,twitter:@EWildn,telegram:t.me/Dubie_420",
    policy: "https://github.com/Dubie-eth/analos-programs/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/Dubie-eth/analos-programs",
    source_revision: "CDJZZCSod3YS9crpWAvWSLWEpPyx9QZCRRAcv7xL1FZf",
    source_release: "v1.0.0",
    auditors: "None",
    acknowledgements: "Thank you to all security researchers who help keep Analos secure!"
}

declare_id!("CDJZZCSod3YS9crpWAvWSLWEpPyx9QZCRRAcv7xL1FZf");

/// Platform fee constants (same as NFT Launchpad for consistency)
pub const FEE_DEV_TEAM_BPS: u16 = 100;           // 1% to dev team
pub const FEE_POOL_CREATION_BPS: u16 = 200;      // 2% for pool creation after bond
pub const FEE_LOL_BUYBACK_BURN_BPS: u16 = 100;   // 1% for LOL buyback and burns
pub const FEE_PLATFORM_MAINT_BPS: u16 = 100;     // 1% for platform maintenance
pub const FEE_LOL_COMMUNITY_BPS: u16 = 100;      // 1% for LOL community rewards
pub const FEE_TOTAL_BPS: u16 = 600;              // 6% total to LOL ecosystem

/// Allocation split (69% pool, 25% creator, 6% fees)
pub const POOL_ALLOCATION_BPS: u16 = 6900;       // 69% to pool
pub const CREATOR_TOTAL_BPS: u16 = 2500;         // 25% to creator

/// Default token configuration
pub const DEFAULT_TOKENS_PER_NFT: u64 = 10_000; // 10,000 tokens per NFT
pub const DEFAULT_DECIMALS: u8 = 6;              // Standard SPL token decimals
pub const MAX_RARITY_TIERS: usize = 10;          // Maximum 10 rarity tiers

/// Creator vesting configuration (10% immediate, 15% vested over 1 year)
pub const CREATOR_IMMEDIATE_CLAIM_BPS: u16 = 1000;  // 10% immediately upon bonding (of 25% total)
pub const CREATOR_VESTED_CLAIM_BPS: u16 = 1500;     // 15% vested over 12 months (of 25% total)
pub const CREATOR_TOTAL_ALLOCATION_BPS: u16 = 2500; // 25% total
pub const CREATOR_VESTING_MONTHS: u64 = 12;         // 12 months vesting for remaining 15%
pub const SECONDS_PER_MONTH: i64 = 30 * 24 * 60 * 60; // ~30 days

/// Creator pre-buy configuration
pub const MAX_CREATOR_PREBUY_BPS: u16 = 500;        // Max 5% of supply
pub const CREATOR_PREBUY_DISCOUNT_BPS: u16 = 1000;  // 10% discount from first BC tier

/// Fee recipient wallets (same as NFT Launchpad)
pub const PLATFORM_FEE_WALLET: Pubkey = pubkey!("myHsakbfHT7x378AvYJkBCtmF3TiSBpxA6DADRExa7Q");
pub const BUYBACK_FEE_WALLET: Pubkey = pubkey!("7V2YgSfqu5E7nx2SXzHzaMPDnxzfh2dNXgBswknvj721");
pub const DEV_FEE_WALLET: Pubkey = pubkey!("Em26WavfAndcLGMWZHakvJHF5iAseHuvsbPXgCDcf63D");

#[program]
pub mod analos_token_launch {
    use super::*;

    /// Initialize token launch configuration for an NFT collection
    pub fn initialize_token_launch(
        ctx: Context<InitializeTokenLaunch>,
        tokens_per_nft: u64,
        pool_percentage_bps: u16,
        token_name: String,
        token_symbol: String,
    ) -> Result<()> {
        let config = &mut ctx.accounts.token_launch_config;
        
        require!(tokens_per_nft > 0, ErrorCode::InvalidTokensPerNFT);
        require!(pool_percentage_bps <= 10000, ErrorCode::InvalidPercentage);
        require!(pool_percentage_bps + (10000 - pool_percentage_bps) == 10000, ErrorCode::InvalidPercentage);
        
        config.nft_collection_config = ctx.accounts.nft_collection_config.key();
        config.token_mint = ctx.accounts.token_mint.key();
        config.token_escrow = ctx.accounts.token_escrow.key();
        config.authority = ctx.accounts.authority.key();
        
        config.tokens_per_nft = tokens_per_nft;
        config.total_tokens_minted = 0;
        config.total_tokens_distributed = 0;
        
        config.pool_percentage_bps = pool_percentage_bps;
        config.creator_percentage_bps = 10000 - pool_percentage_bps;
        config.pool_tokens = 0;
        config.creator_tokens = 0;
        
        config.dlmm_pool = None;
        config.dlmm_position = None;
        config.is_bonded = false;
        config.bond_time = None;
        
        config.buyback_enabled = false;
        config.buyback_price_tokens = 0;
        config.total_buybacks = 0;
        
        config.token_name = token_name.clone();
        config.token_symbol = token_symbol.clone();
        config.created_at = Clock::get()?.unix_timestamp;
        
        emit!(TokenLaunchInitializedEvent {
            nft_collection: ctx.accounts.nft_collection_config.key(),
            token_mint: ctx.accounts.token_mint.key(),
            tokens_per_nft,
            pool_percentage_bps,
            token_name,
            token_symbol,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Token launch initialized: {} tokens per NFT, {}% to pool", 
            tokens_per_nft, pool_percentage_bps / 100);
        
        Ok(())
    }

    /// Mint tokens when an NFT is minted (called via CPI from NFT Launchpad)
    pub fn mint_tokens_for_nft(
        ctx: Context<MintTokensForNFT>,
        nft_mint: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.token_launch_config;
        
        // Mint base tokens to escrow
        let tokens_to_mint = config.tokens_per_nft * 10u64.pow(DEFAULT_DECIMALS as u32);
        
        let seeds = &[
            b"token_launch_config".as_ref(),
            config.nft_collection_config.as_ref(),
            &[ctx.bumps.token_launch_config],
        ];
        let signer_seeds = &[&seeds[..]];
        
        let cpi_accounts = MintTo {
            mint: ctx.accounts.token_mint.to_account_info(),
            to: ctx.accounts.token_escrow.to_account_info(),
            authority: config.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer_seeds,
        );
        token::mint_to(cpi_ctx, tokens_to_mint)?;
        
        config.total_tokens_minted += tokens_to_mint;
        
        emit!(TokensMintedForNFTEvent {
            nft_mint,
            tokens_minted: tokens_to_mint,
            total_tokens_minted: config.total_tokens_minted,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Minted {} tokens for NFT {}", tokens_to_mint, nft_mint);
        
        Ok(())
    }

    /// Distribute tokens to user based on rarity (called after reveal)
    pub fn distribute_tokens_by_rarity(
        ctx: Context<DistributeTokens>,
        nft_mint: Pubkey,
        rarity_tier: u8,
        token_multiplier: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.token_launch_config;
        let user_claim = &mut ctx.accounts.user_token_claim;
        
        require!(rarity_tier < MAX_RARITY_TIERS as u8, ErrorCode::InvalidRarityTier);
        require!(token_multiplier > 0 && token_multiplier <= 1000, ErrorCode::InvalidMultiplier);
        
        // Calculate tokens to distribute
        let base_tokens = config.tokens_per_nft * 10u64.pow(DEFAULT_DECIMALS as u32);
        let tokens_to_distribute = base_tokens.checked_mul(token_multiplier)
            .ok_or(ErrorCode::MathOverflow)?;
        
        // Transfer tokens from escrow to user
        let seeds = &[
            b"token_launch_config".as_ref(),
            config.nft_collection_config.as_ref(),
            &[ctx.bumps.token_launch_config],
        ];
        let signer_seeds = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.token_escrow.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: config.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer_seeds,
        );
        token::transfer(cpi_ctx, tokens_to_distribute)?;
        
        config.total_tokens_distributed += tokens_to_distribute;
        
        // Record claim
        user_claim.user = ctx.accounts.user.key();
        user_claim.collection_config = config.nft_collection_config;
        user_claim.nft_mint = nft_mint;
        user_claim.rarity_tier = rarity_tier;
        user_claim.tokens_claimed = tokens_to_distribute;
        user_claim.token_multiplier = token_multiplier;
        user_claim.claimed_at = Clock::get()?.unix_timestamp;
        
        emit!(TokensDistributedEvent {
            user: ctx.accounts.user.key(),
            nft_mint,
            rarity_tier,
            token_multiplier,
            tokens_distributed: tokens_to_distribute,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Distributed {} tokens ({}x multiplier) to user for rarity tier {}", 
            tokens_to_distribute, token_multiplier, rarity_tier);
        
        Ok(())
    }

    /// Trigger bonding and create DLMM pool (called when collection sells out)
    pub fn trigger_bonding(
        ctx: Context<TriggerBonding>,
        initial_sol_amount: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.token_launch_config;
        
        require!(!config.is_bonded, ErrorCode::AlreadyBonded);
        require!(initial_sol_amount > 0, ErrorCode::InvalidSOLAmount);
        
        // Calculate pool (69%) vs creator (25%) allocation (6% already distributed as fees)
        let total_tokens_in_escrow = ctx.accounts.token_escrow.amount;
        config.pool_tokens = total_tokens_in_escrow * POOL_ALLOCATION_BPS as u64 / 10000; // 69%
        config.creator_tokens = total_tokens_in_escrow * CREATOR_TOTAL_BPS as u64 / 10000; // 25%
        
        // NEW: Calculate vesting schedule (10% immediate, 15% vested over 12 months)
        config.creator_immediate_tokens = config.creator_tokens * CREATOR_IMMEDIATE_CLAIM_BPS as u64 / CREATOR_TOTAL_BPS as u64; // 10% of 25%
        config.creator_vested_tokens = config.creator_tokens - config.creator_immediate_tokens; // 15% of 25%
        config.creator_tokens_claimed = 0;
        config.creator_vesting_start = Some(Clock::get()?.unix_timestamp);
        config.vesting_duration_months = CREATOR_VESTING_MONTHS; // 12 months
        
        // Mark as bonded
        config.is_bonded = true;
        config.bond_time = Some(Clock::get()?.unix_timestamp);
        
        // Enable buyback
        config.buyback_enabled = true;
        
        emit!(BondingTriggeredEvent {
            collection_config: config.nft_collection_config,
            pool_tokens: config.pool_tokens,
            creator_tokens: config.creator_tokens,
            initial_sol_amount,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        emit!(CreatorVestingStartedEvent {
            collection_config: config.nft_collection_config,
            total_creator_tokens: config.creator_tokens,
            immediate_tokens: config.creator_immediate_tokens,
            vested_tokens: config.creator_vested_tokens,
            vesting_months: config.vesting_duration_months,
            vesting_start: Clock::get()?.unix_timestamp,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Bonding triggered! Pool: {} tokens (69%), Creator: {} tokens (25% - 10% immediate, 15% vested over 12 months)", 
            config.pool_tokens, config.creator_tokens);
        
        // Note: Actual DLMM pool creation would happen in a separate instruction
        // due to complexity and need to interact with Meteora program
        
        Ok(())
    }

    /// Set DLMM pool address after creation
    pub fn set_dlmm_pool(
        ctx: Context<SetDLMMPool>,
        dlmm_pool: Pubkey,
        dlmm_position: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.token_launch_config;
        
        require!(config.is_bonded, ErrorCode::NotBonded);
        require!(config.dlmm_pool.is_none(), ErrorCode::PoolAlreadySet);
        
        config.dlmm_pool = Some(dlmm_pool);
        config.dlmm_position = Some(dlmm_position);
        
        emit!(DLMMPoolSetEvent {
            collection_config: config.nft_collection_config,
            dlmm_pool,
            dlmm_position,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("DLMM pool set: {}", dlmm_pool);
        
        Ok(())
    }

    /// Configure buyback pricing
    pub fn configure_buyback(
        ctx: Context<ConfigureBuyback>,
        enabled: bool,
        price_tokens: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.token_launch_config;
        
        require!(config.is_bonded, ErrorCode::NotBonded);
        
        config.buyback_enabled = enabled;
        config.buyback_price_tokens = price_tokens;
        
        emit!(BuybackConfiguredEvent {
            collection_config: config.nft_collection_config,
            enabled,
            price_tokens,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Buyback configured: enabled={}, price={} tokens", enabled, price_tokens);
        
        Ok(())
    }

    /// Buyback NFT with tokens (burn tokens, user gets to re-mint)
    pub fn buyback_nft_with_tokens(
        ctx: Context<BuybackNFT>,
    ) -> Result<()> {
        let config = &mut ctx.accounts.token_launch_config;
        
        require!(config.buyback_enabled, ErrorCode::BuybackDisabled);
        require!(config.buyback_price_tokens > 0, ErrorCode::InvalidBuybackPrice);
        
        // Burn tokens from user
        let cpi_accounts = Burn {
            mint: ctx.accounts.token_mint.to_account_info(),
            from: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
        );
        token::burn(cpi_ctx, config.buyback_price_tokens)?;
        
        config.total_buybacks += 1;
        
        emit!(NFTBoughtBackEvent {
            user: ctx.accounts.user.key(),
            tokens_burned: config.buyback_price_tokens,
            total_buybacks: config.total_buybacks,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("User {} bought back NFT for {} tokens", 
            ctx.accounts.user.key(), config.buyback_price_tokens);
        
        // Note: Actual NFT re-minting would be done via CPI to NFT Launchpad program
        
        Ok(())
    }

    /// Withdraw creator tokens with vesting (5% immediate, 15% vested over 6 months)
    pub fn withdraw_creator_tokens(
        ctx: Context<WithdrawCreatorTokens>,
        amount: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.token_launch_config;
        
        require!(config.is_bonded, ErrorCode::NotBonded);
        
        // Calculate available tokens
        let current_time = Clock::get()?.unix_timestamp;
        let vesting_start = config.creator_vesting_start.ok_or(ErrorCode::VestingNotStarted)?;
        
        // Calculate vested amount
        let elapsed_time = current_time - vesting_start;
        let elapsed_months = elapsed_time / SECONDS_PER_MONTH;
        let vesting_months = config.vesting_duration_months as i64;
        
        let vested_tokens = if elapsed_months >= vesting_months {
            // Fully vested
            config.creator_vested_tokens
        } else {
            // Partially vested (linear)
            config.creator_vested_tokens * elapsed_months as u64 / vesting_months as u64
        };
        
        // Total available = immediate (5%) + vested amount - already claimed
        let total_available = config.creator_immediate_tokens + vested_tokens - config.creator_tokens_claimed;
        
        require!(amount <= total_available, ErrorCode::InsufficientVestedTokens);
        
        let seeds = &[
            b"token_launch_config".as_ref(),
            config.nft_collection_config.as_ref(),
            &[ctx.bumps.token_launch_config],
        ];
        let signer_seeds = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.token_escrow.to_account_info(),
            to: ctx.accounts.creator_token_account.to_account_info(),
            authority: config.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer_seeds,
        );
        token::transfer(cpi_ctx, amount)?;
        
        config.creator_tokens_claimed += amount;
        config.creator_last_claim = Some(current_time);
        
        emit!(CreatorTokensWithdrawnEvent {
            creator: ctx.accounts.creator.key(),
            amount,
            total_claimed: config.creator_tokens_claimed,
            remaining_immediate: config.creator_immediate_tokens.saturating_sub(config.creator_tokens_claimed),
            remaining_vested: vested_tokens.saturating_sub(config.creator_tokens_claimed.saturating_sub(config.creator_immediate_tokens)),
            months_elapsed: elapsed_months as u64,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Creator withdrew {} tokens ({} months vested, {} total claimed)", 
            amount, elapsed_months, config.creator_tokens_claimed);
        
        Ok(())
    }

    /// Creator pre-buy tokens at discount (before bonding)
    pub fn creator_prebuy_tokens(
        ctx: Context<CreatorPrebuyTokens>,
        amount_tokens: u64,
        payment_sol: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.token_launch_config;
        
        require!(!config.is_bonded, ErrorCode::AlreadyBonded);
        require!(config.creator_prebuy_enabled, ErrorCode::PrebuyDisabled);
        
        // Check max prebuy limit (5% of total supply)
        let max_prebuy = config.total_tokens_minted * MAX_CREATOR_PREBUY_BPS as u64 / 10000;
        require!(
            config.creator_prebuy_amount + amount_tokens <= max_prebuy,
            ErrorCode::PrebuyLimitExceeded
        );
        
        // Transfer SOL payment to escrow
        **ctx.accounts.creator.to_account_info().try_borrow_mut_lamports()? -= payment_sol;
        **ctx.accounts.token_escrow.to_account_info().try_borrow_mut_lamports()? += payment_sol;
        
        // Transfer tokens to creator
        let seeds = &[
            b"token_launch_config".as_ref(),
            config.nft_collection_config.as_ref(),
            &[ctx.bumps.token_launch_config],
        ];
        let signer_seeds = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.token_escrow.to_account_info(),
            to: ctx.accounts.creator_token_account.to_account_info(),
            authority: config.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer_seeds,
        );
        token::transfer(cpi_ctx, amount_tokens)?;
        
        config.creator_prebuy_amount += amount_tokens;
        
        emit!(CreatorPrebuyEvent {
            creator: ctx.accounts.creator.key(),
            tokens_bought: amount_tokens,
            sol_paid: payment_sol,
            total_prebuy: config.creator_prebuy_amount,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Creator pre-bought {} tokens for {} SOL", amount_tokens, payment_sol);
        
        Ok(())
    }

    /// Claim trading fees from bonding curve (available anytime)
    pub fn claim_trading_fees(
        ctx: Context<ClaimTradingFees>,
        amount: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.token_launch_config;
        
        let available_fees = config.trading_fees_collected - config.trading_fees_claimed;
        require!(amount <= available_fees, ErrorCode::InsufficientTradingFees);
        
        // Transfer SOL fees to creator
        **ctx.accounts.token_escrow.to_account_info().try_borrow_mut_lamports()? -= amount;
        **ctx.accounts.creator.to_account_info().try_borrow_mut_lamports()? += amount;
        
        config.trading_fees_claimed += amount;
        
        emit!(TradingFeesClaimedEvent {
            creator: ctx.accounts.creator.key(),
            amount,
            total_claimed: config.trading_fees_claimed,
            remaining: available_fees - amount,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Creator claimed {} SOL in trading fees", amount);
        
        Ok(())
    }
}

// ========== ACCOUNT CONTEXTS ==========

#[derive(Accounts)]
pub struct InitializeTokenLaunch<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + TokenLaunchConfig::INIT_SPACE,
        seeds = [b"token_launch_config", nft_collection_config.key().as_ref()],
        bump
    )]
    pub token_launch_config: Account<'info, TokenLaunchConfig>,

    /// CHECK: NFT collection config from NFT Launchpad program
    pub nft_collection_config: UncheckedAccount<'info>,

    #[account(
        init,
        payer = authority,
        mint::decimals = DEFAULT_DECIMALS,
        mint::authority = token_launch_config,
    )]
    pub token_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = authority,
        token::mint = token_mint,
        token::authority = token_launch_config,
    )]
    pub token_escrow: Account<'info, TokenAccount>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct MintTokensForNFT<'info> {
    #[account(
        mut,
        seeds = [b"token_launch_config", token_launch_config.nft_collection_config.as_ref()],
        bump,
    )]
    pub token_launch_config: Account<'info, TokenLaunchConfig>,

    #[account(mut)]
    pub token_mint: Account<'info, Mint>,

    #[account(mut)]
    pub token_escrow: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct DistributeTokens<'info> {
    #[account(
        mut,
        seeds = [b"token_launch_config", token_launch_config.nft_collection_config.as_ref()],
        bump,
    )]
    pub token_launch_config: Account<'info, TokenLaunchConfig>,

    #[account(
        init,
        payer = user,
        space = 8 + UserTokenClaim::INIT_SPACE,
        seeds = [b"user_token_claim", token_launch_config.key().as_ref(), user.key().as_ref()],
        bump
    )]
    pub user_token_claim: Account<'info, UserTokenClaim>,

    #[account(mut)]
    pub token_escrow: Account<'info, TokenAccount>,

    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct TriggerBonding<'info> {
    #[account(
        mut,
        seeds = [b"token_launch_config", token_launch_config.nft_collection_config.as_ref()],
        bump,
        has_one = authority,
    )]
    pub token_launch_config: Account<'info, TokenLaunchConfig>,

    #[account(mut)]
    pub token_escrow: Account<'info, TokenAccount>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetDLMMPool<'info> {
    #[account(
        mut,
        seeds = [b"token_launch_config", token_launch_config.nft_collection_config.as_ref()],
        bump,
        has_one = authority,
    )]
    pub token_launch_config: Account<'info, TokenLaunchConfig>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct ConfigureBuyback<'info> {
    #[account(
        mut,
        seeds = [b"token_launch_config", token_launch_config.nft_collection_config.as_ref()],
        bump,
        has_one = authority,
    )]
    pub token_launch_config: Account<'info, TokenLaunchConfig>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct BuybackNFT<'info> {
    #[account(
        mut,
        seeds = [b"token_launch_config", token_launch_config.nft_collection_config.as_ref()],
        bump,
    )]
    pub token_launch_config: Account<'info, TokenLaunchConfig>,

    #[account(mut)]
    pub token_mint: Account<'info, Mint>,

    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,

    pub user: Signer<'info>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct WithdrawCreatorTokens<'info> {
    #[account(
        mut,
        seeds = [b"token_launch_config", token_launch_config.nft_collection_config.as_ref()],
        bump,
        has_one = authority,
    )]
    pub token_launch_config: Account<'info, TokenLaunchConfig>,

    #[account(mut)]
    pub token_escrow: Account<'info, TokenAccount>,

    #[account(mut)]
    pub creator_token_account: Account<'info, TokenAccount>,

    pub creator: Signer<'info>,
    pub authority: Signer<'info>,

    pub token_program: Program<'info, Token>,
}

// ========== STATE ==========

#[account]
#[derive(InitSpace)]
pub struct TokenLaunchConfig {
    pub nft_collection_config: Pubkey,
    pub token_mint: Pubkey,
    pub token_escrow: Pubkey,
    pub authority: Pubkey,
    
    pub tokens_per_nft: u64,
    pub total_tokens_minted: u64,
    pub total_tokens_distributed: u64,
    
    pub pool_percentage_bps: u16,
    pub creator_percentage_bps: u16,
    pub pool_tokens: u64,
    pub creator_tokens: u64,
    
    pub dlmm_pool: Option<Pubkey>,
    pub dlmm_position: Option<Pubkey>,
    pub is_bonded: bool,
    pub bond_time: Option<i64>,
    
    pub buyback_enabled: bool,
    pub buyback_price_tokens: u64,
    pub total_buybacks: u64,
    
    #[max_len(32)]
    pub token_name: String,
    #[max_len(10)]
    pub token_symbol: String,
    pub created_at: i64,
    
    // NEW: Creator vesting
    pub creator_vesting_start: Option<i64>,        // When vesting starts (bonding time)
    pub creator_immediate_tokens: u64,             // 5% claimable immediately
    pub creator_vested_tokens: u64,                // 15% vested over 6 months
    pub creator_tokens_claimed: u64,               // Total claimed so far
    pub creator_last_claim: Option<i64>,           // Last claim timestamp
    pub vesting_duration_months: u64,              // Vesting period (6 months)
    
    // NEW: Creator pre-buy
    pub creator_prebuy_enabled: bool,              // Allow pre-buy?
    pub creator_prebuy_max_bps: u16,              // Max % of supply (500 = 5%)
    pub creator_prebuy_discount_bps: u16,          // Discount from BC (1000 = 10%)
    pub creator_prebuy_amount: u64,                // Amount pre-bought
    
    // NEW: Trading fee accumulation
    pub trading_fees_collected: u64,               // Fees from bonding curve trades
    pub trading_fees_claimed: u64,                 // Fees claimed by creator
}

#[account]
#[derive(InitSpace)]
pub struct UserTokenClaim {
    pub user: Pubkey,
    pub collection_config: Pubkey,
    pub nft_mint: Pubkey,
    pub rarity_tier: u8,
    pub tokens_claimed: u64,
    pub token_multiplier: u64,
    pub claimed_at: i64,
}

// ========== EVENTS ==========

#[event]
pub struct TokenLaunchInitializedEvent {
    pub nft_collection: Pubkey,
    pub token_mint: Pubkey,
    pub tokens_per_nft: u64,
    pub pool_percentage_bps: u16,
    pub token_name: String,
    pub token_symbol: String,
    pub timestamp: i64,
}

#[event]
pub struct TokensMintedForNFTEvent {
    pub nft_mint: Pubkey,
    pub tokens_minted: u64,
    pub total_tokens_minted: u64,
    pub timestamp: i64,
}

#[event]
pub struct TokensDistributedEvent {
    pub user: Pubkey,
    pub nft_mint: Pubkey,
    pub rarity_tier: u8,
    pub token_multiplier: u64,
    pub tokens_distributed: u64,
    pub timestamp: i64,
}

#[event]
pub struct BondingTriggeredEvent {
    pub collection_config: Pubkey,
    pub pool_tokens: u64,
    pub creator_tokens: u64,
    pub initial_sol_amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct DLMMPoolSetEvent {
    pub collection_config: Pubkey,
    pub dlmm_pool: Pubkey,
    pub dlmm_position: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct BuybackConfiguredEvent {
    pub collection_config: Pubkey,
    pub enabled: bool,
    pub price_tokens: u64,
    pub timestamp: i64,
}

#[event]
pub struct NFTBoughtBackEvent {
    pub user: Pubkey,
    pub tokens_burned: u64,
    pub total_buybacks: u64,
    pub timestamp: i64,
}

#[event]
pub struct CreatorTokensWithdrawnEvent {
    pub creator: Pubkey,
    pub amount: u64,
    pub total_claimed: u64,
    pub remaining_immediate: u64,
    pub remaining_vested: u64,
    pub months_elapsed: u64,
    pub timestamp: i64,
}

#[event]
pub struct CreatorVestingStartedEvent {
    pub collection_config: Pubkey,
    pub total_creator_tokens: u64,
    pub immediate_tokens: u64,
    pub vested_tokens: u64,
    pub vesting_months: u64,
    pub vesting_start: i64,
    pub timestamp: i64,
}

#[event]
pub struct CreatorPrebuyEvent {
    pub creator: Pubkey,
    pub tokens_bought: u64,
    pub sol_paid: u64,
    pub total_prebuy: u64,
    pub timestamp: i64,
}

#[event]
pub struct TradingFeesClaimedEvent {
    pub creator: Pubkey,
    pub amount: u64,
    pub total_claimed: u64,
    pub remaining: u64,
    pub timestamp: i64,
}

// ========== ERRORS ==========

#[error_code]
pub enum ErrorCode {
    #[msg("Invalid tokens per NFT")]
    InvalidTokensPerNFT,
    #[msg("Invalid percentage (must be 0-10000)")]
    InvalidPercentage,
    #[msg("Invalid rarity tier")]
    InvalidRarityTier,
    #[msg("Invalid multiplier (1-1000)")]
    InvalidMultiplier,
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Already bonded")]
    AlreadyBonded,
    #[msg("Not bonded yet")]
    NotBonded,
    #[msg("Invalid SOL amount")]
    InvalidSOLAmount,
    #[msg("Pool already set")]
    PoolAlreadySet,
    #[msg("Buyback disabled")]
    BuybackDisabled,
    #[msg("Invalid buyback price")]
    InvalidBuybackPrice,
    #[msg("Insufficient creator tokens")]
    InsufficientCreatorTokens,
    #[msg("Vesting not started")]
    VestingNotStarted,
    #[msg("Insufficient vested tokens available")]
    InsufficientVestedTokens,
    #[msg("Pre-buy disabled")]
    PrebuyDisabled,
    #[msg("Pre-buy limit exceeded (max 5%)")]
    PrebuyLimitExceeded,
    #[msg("Insufficient trading fees")]
    InsufficientTradingFees,
}

// ========== ACCOUNT CONTEXTS (NEW) ==========

#[derive(Accounts)]
pub struct CreatorPrebuyTokens<'info> {
    #[account(
        mut,
        seeds = [b"token_launch_config", token_launch_config.nft_collection_config.as_ref()],
        bump,
        has_one = authority,
    )]
    pub token_launch_config: Account<'info, TokenLaunchConfig>,

    #[account(mut)]
    pub token_escrow: Account<'info, TokenAccount>,

    #[account(mut)]
    pub creator_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub creator: Signer<'info>,
    pub authority: Signer<'info>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ClaimTradingFees<'info> {
    #[account(
        mut,
        seeds = [b"token_launch_config", token_launch_config.nft_collection_config.as_ref()],
        bump,
        has_one = authority,
    )]
    pub token_launch_config: Account<'info, TokenLaunchConfig>,

    /// CHECK: Token escrow (holds SOL from trading fees)
    #[account(mut)]
    pub token_escrow: UncheckedAccount<'info>,

    #[account(mut)]
    pub creator: Signer<'info>,
    pub authority: Signer<'info>,
}

