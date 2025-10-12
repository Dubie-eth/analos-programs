use anchor_lang::prelude::*;

// Security.txt implementation for program verification
#[cfg(not(feature = "no-entrypoint"))]
use {default_env::default_env, solana_security_txt::security_txt};

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Analos Price Oracle",
    project_url: "https://github.com/Dubie-eth/analos-programs",
    contacts: "email:security@analos.io,twitter:@EWildn,telegram:t.me/Dubie_420",
    policy: "https://github.com/Dubie-eth/analos-programs/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/Dubie-eth/analos-programs",
    source_revision: "5ihyquuoRJXTocBhjEA48rGQGsM9ZB6HezYE1dQq8NUD",
    source_release: "v1.0.0",
    auditors: "None",
    acknowledgements: "Thank you to all security researchers who help keep Analos secure!"
}

declare_id!("5ihyquuoRJXTocBhjEA48rGQGsM9ZB6HezYE1dQq8NUD");

/// Price oracle constants
pub const PRICE_UPDATE_TOLERANCE_BPS: u16 = 1000; // 10% max change per update
pub const MAX_PRICE_STALENESS_SECONDS: i64 = 300; // 5 minutes max age
pub const DECIMALS_USD: u8 = 6; // USD with 6 decimals ($1.00 = 1,000,000)
pub const DECIMALS_LOS: u8 = 9; // LOS with 9 decimals (1 LOS = 1,000,000,000)

#[program]
pub mod analos_price_oracle {
    use super::*;

    /// Initialize price oracle
    pub fn initialize_oracle(
        ctx: Context<InitializeOracle>,
        initial_los_market_cap_usd: u64,
    ) -> Result<()> {
        let oracle = &mut ctx.accounts.price_oracle;
        
        oracle.authority = ctx.accounts.authority.key();
        oracle.los_market_cap_usd = initial_los_market_cap_usd;
        oracle.los_price_usd = 0; // Will be calculated
        oracle.last_update = Clock::get()?.unix_timestamp;
        oracle.update_count = 0;
        oracle.is_active = true;
        
        emit!(OracleInitializedEvent {
            authority: oracle.authority,
            initial_los_market_cap_usd,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Price oracle initialized with $LOS market cap: ${}", initial_los_market_cap_usd);
        
        Ok(())
    }

    /// Update $LOS market cap (authorized updater only)
    pub fn update_los_market_cap(
        ctx: Context<UpdateLOSPrice>,
        new_market_cap_usd: u64,
        los_circulating_supply: u64,
    ) -> Result<()> {
        let oracle = &mut ctx.accounts.price_oracle;
        
        require!(oracle.is_active, ErrorCode::OracleInactive);
        
        // Calculate max allowed change (10% tolerance)
        let max_increase = oracle.los_market_cap_usd + (oracle.los_market_cap_usd * PRICE_UPDATE_TOLERANCE_BPS as u64 / 10000);
        let max_decrease = oracle.los_market_cap_usd.saturating_sub(oracle.los_market_cap_usd * PRICE_UPDATE_TOLERANCE_BPS as u64 / 10000);
        
        // For emergency price crashes, allow bigger changes with multi-sig
        if new_market_cap_usd > max_increase || new_market_cap_usd < max_decrease {
            // Require emergency authority (could be multi-sig)
            require!(
                ctx.accounts.updater.key() == oracle.authority,
                ErrorCode::PriceChangeTooBig
            );
        }
        
        let old_market_cap = oracle.los_market_cap_usd;
        oracle.los_market_cap_usd = new_market_cap_usd;
        
        // Calculate $LOS price in USD (market_cap / circulating_supply)
        if los_circulating_supply > 0 {
            oracle.los_price_usd = new_market_cap_usd * 10u64.pow(DECIMALS_LOS as u32) / los_circulating_supply;
        }
        
        oracle.last_update = Clock::get()?.unix_timestamp;
        oracle.update_count += 1;
        
        emit!(LOSPriceUpdatedEvent {
            old_market_cap_usd: old_market_cap,
            new_market_cap_usd,
            los_price_usd: oracle.los_price_usd,
            circulating_supply: los_circulating_supply,
            updated_by: ctx.accounts.updater.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("$LOS market cap updated: ${} → ${}, Price: ${}", 
            old_market_cap, new_market_cap_usd, oracle.los_price_usd);
        
        Ok(())
    }

    /// Calculate USD-pegged price in LOS (for NFTs, tokens, etc.)
    pub fn calculate_los_amount_for_usd(
        ctx: Context<CalculatePrice>,
        usd_amount: u64,
    ) -> Result<()> {
        let oracle = &ctx.accounts.price_oracle;
        
        require!(oracle.is_active, ErrorCode::OracleInactive);
        
        // Check price staleness
        let current_time = Clock::get()?.unix_timestamp;
        require!(
            current_time - oracle.last_update < MAX_PRICE_STALENESS_SECONDS,
            ErrorCode::PriceTooStale
        );
        
        require!(oracle.los_price_usd > 0, ErrorCode::InvalidPrice);
        
        // Calculate LOS amount needed for USD value
        // usd_amount (with 6 decimals) / los_price_usd (with 6 decimals) * 10^9 (LOS decimals)
        let los_lamports = usd_amount * 10u64.pow(DECIMALS_LOS as u32) / oracle.los_price_usd;
        
        msg!("${} USD = {} lamports ($LOS price: ${})", 
            usd_amount, los_lamports, oracle.los_price_usd);
        
        Ok(())
    }

    /// Get current pool initial market cap target in LOS
    pub fn calculate_pool_target_los(
        ctx: Context<CalculatePoolTarget>,
        target_market_cap_usd: u64,
    ) -> Result<()> {
        let oracle = &ctx.accounts.price_oracle;
        
        require!(oracle.is_active, ErrorCode::OracleInactive);
        require!(oracle.los_price_usd > 0, ErrorCode::InvalidPrice);
        
        // Calculate how much LOS needed for target market cap
        let los_needed = target_market_cap_usd * 10u64.pow(DECIMALS_LOS as u32) / oracle.los_price_usd;
        
        msg!("To achieve ${} market cap, need {} LOS", target_market_cap_usd, los_needed);
        
        Ok(())
    }

    /// Emergency: Pause oracle (security)
    pub fn pause_oracle(
        ctx: Context<PauseOracle>,
        reason: String,
    ) -> Result<()> {
        let oracle = &mut ctx.accounts.price_oracle;
        
        oracle.is_active = false;
        
        emit!(OraclePausedEvent {
            reason,
            paused_by: ctx.accounts.authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("⚠️  Oracle paused: {}", reason);
        
        Ok(())
    }

    /// Resume oracle
    pub fn resume_oracle(
        ctx: Context<ResumeOracle>,
    ) -> Result<()> {
        let oracle = &mut ctx.accounts.price_oracle;
        
        oracle.is_active = true;
        
        emit!(OracleResumedEvent {
            resumed_by: ctx.accounts.authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("✅ Oracle resumed");
        
        Ok(())
    }
}

// ========== ACCOUNT CONTEXTS ==========

#[derive(Accounts)]
pub struct InitializeOracle<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + PriceOracle::INIT_SPACE,
        seeds = [b"price_oracle"],
        bump
    )]
    pub price_oracle: Account<'info, PriceOracle>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateLOSPrice<'info> {
    #[account(
        mut,
        seeds = [b"price_oracle"],
        bump,
    )]
    pub price_oracle: Account<'info, PriceOracle>,

    pub updater: Signer<'info>,
}

#[derive(Accounts)]
pub struct CalculatePrice<'info> {
    #[account(
        seeds = [b"price_oracle"],
        bump,
    )]
    pub price_oracle: Account<'info, PriceOracle>,
}

#[derive(Accounts)]
pub struct CalculatePoolTarget<'info> {
    #[account(
        seeds = [b"price_oracle"],
        bump,
    )]
    pub price_oracle: Account<'info, PriceOracle>,
}

#[derive(Accounts)]
pub struct PauseOracle<'info> {
    #[account(
        mut,
        seeds = [b"price_oracle"],
        bump,
        has_one = authority,
    )]
    pub price_oracle: Account<'info, PriceOracle>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct ResumeOracle<'info> {
    #[account(
        mut,
        seeds = [b"price_oracle"],
        bump,
        has_one = authority,
    )]
    pub price_oracle: Account<'info, PriceOracle>,

    pub authority: Signer<'info>,
}

// ========== STATE ==========

#[account]
#[derive(InitSpace)]
pub struct PriceOracle {
    pub authority: Pubkey,
    pub los_market_cap_usd: u64,        // $LOS market cap in USD (6 decimals)
    pub los_price_usd: u64,             // $LOS price in USD (6 decimals)
    pub last_update: i64,               // Last update timestamp
    pub update_count: u64,              // Number of updates
    pub is_active: bool,                // Oracle active?
}

// ========== EVENTS ==========

#[event]
pub struct OracleInitializedEvent {
    pub authority: Pubkey,
    pub initial_los_market_cap_usd: u64,
    pub timestamp: i64,
}

#[event]
pub struct LOSPriceUpdatedEvent {
    pub old_market_cap_usd: u64,
    pub new_market_cap_usd: u64,
    pub los_price_usd: u64,
    pub circulating_supply: u64,
    pub updated_by: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct OraclePausedEvent {
    pub reason: String,
    pub paused_by: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct OracleResumedEvent {
    pub resumed_by: Pubkey,
    pub timestamp: i64,
}

// ========== ERRORS ==========

#[error_code]
pub enum ErrorCode {
    #[msg("Oracle is inactive")]
    OracleInactive,
    #[msg("Price change too big (max 10% per update)")]
    PriceChangeTooBig,
    #[msg("Price data is too stale (max 5 minutes)")]
    PriceTooStale,
    #[msg("Invalid price")]
    InvalidPrice,
}

