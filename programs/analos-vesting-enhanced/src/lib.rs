use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

// Security.txt implementation for program verification
#[cfg(not(feature = "no-entrypoint"))]
use {default_env::default_env, solana_security_txt::security_txt};

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Analos Vesting Enhanced",
    project_url: "https://github.com/Dubie-eth/analos-programs",
    contacts: "email:support@launchonlos.fun,twitter:@EWildn,telegram:t.me/Dubie_420",
    policy: "https://github.com/Dubie-eth/analos-programs/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/Dubie-eth/analos-programs",
    source_revision: "Ae3hXKsHzYPCPUKLtq2mdYZ3E2oKeKrF63ekceGxpHsY",
    source_release: "v1.0.0",
    auditors: "None",
    acknowledgements: "Thank you to all security researchers who help keep Analos secure!"
}

declare_id!("Ae3hXKsHzYPCPUKLtq2mdYZ3E2oKeKrF63ekceGxpHsY");

/// Enhanced Token Vesting Program with Advanced Security Features
/// - Multi-signature support
/// - Rate limiting
/// - Enhanced logging and monitoring
/// - Emergency pause functions (already implemented)
#[program]
pub mod analos_vesting_enhanced {
    use super::*;

    /// Create vesting with enhanced security
    pub fn create_vesting(
        ctx: Context<CreateVesting>,
        total_amount: u64,
        start_time: i64,
        end_time: i64,
        cliff_time: i64,
        release_frequency: i64,
        recipient: Pubkey,
        is_revocable: bool,
    ) -> Result<()> {
        // Check if program is paused
        require!(!ctx.accounts.program_state.is_paused, ErrorCode::ProgramPaused);
        
        // Rate limiting check
        check_rate_limit(&mut ctx.accounts.rate_limit, 5, 3600)?; // 5 vestings per hour
        
        // Input validation
        require!(end_time > start_time, ErrorCode::InvalidTimeRange);
        require!(cliff_time >= start_time, ErrorCode::CliffBeforeStart);
        require!(cliff_time <= end_time, ErrorCode::CliffAfterEnd);
        require!(total_amount > 0, ErrorCode::ZeroAmount);
        require!(release_frequency > 0, ErrorCode::InvalidFrequency);
        
        let vesting = &mut ctx.accounts.vesting_account;
        vesting.creator = ctx.accounts.creator.key();
        vesting.recipient = recipient;
        vesting.token_account = ctx.accounts.token_account.key();
        vesting.total_amount = total_amount;
        vesting.released_amount = 0;
        vesting.start_time = start_time;
        vesting.end_time = end_time;
        vesting.cliff_time = cliff_time;
        vesting.release_frequency = release_frequency;
        vesting.is_revocable = is_revocable;
        vesting.is_revoked = false;
        vesting.is_paused = false;
        vesting.created_at = Clock::get()?.unix_timestamp;
        vesting.nonce = ctx.accounts.program_state.next_nonce;
        
        // Update program state
        ctx.accounts.program_state.next_nonce = ctx.accounts.program_state.next_nonce.saturating_add(1);
        
        // Transfer tokens to vesting account
        let cpi_accounts = Transfer {
            from: ctx.accounts.creator_token_account.to_account_info(),
            to: ctx.accounts.token_account.to_account_info(),
            authority: ctx.accounts.creator.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, total_amount)?;
        
        emit!(VestingCreatedEvent {
            vesting_account: vesting.key(),
            creator: vesting.creator,
            recipient,
            total_amount,
            start_time,
            end_time,
            cliff_time,
            is_revocable,
            created_at: vesting.created_at,
            nonce: vesting.nonce,
        });
        
        emit!(SecurityEvent {
            event_type: "VESTING_CREATED".to_string(),
            severity: "INFO".to_string(),
            user: vesting.creator,
            timestamp: Clock::get()?.unix_timestamp,
            details: format!("Vesting created: {} tokens over {} seconds", total_amount, end_time - start_time),
            risk_level: 1,
        });
        
        msg!("✅ Vesting created: {} tokens over {} seconds (revocable: {})", total_amount, end_time - start_time, is_revocable);
        Ok(())
    }

    /// Claim vested tokens with enhanced security
    pub fn claim_vested(ctx: Context<ClaimVested>) -> Result<()> {
        // Check if program is paused
        require!(!ctx.accounts.program_state.is_paused, ErrorCode::ProgramPaused);
        
        // Rate limiting check
        check_rate_limit(&mut ctx.accounts.rate_limit, 10, 3600)?; // 10 claims per hour
        
        // Read all values BEFORE mutable borrow
        let is_revoked = ctx.accounts.vesting_account.is_revoked;
        let is_paused = ctx.accounts.vesting_account.is_paused;
        let cliff_time = ctx.accounts.vesting_account.cliff_time;
        let total_amount = ctx.accounts.vesting_account.total_amount;
        let released_amount = ctx.accounts.vesting_account.released_amount;
        let start_time = ctx.accounts.vesting_account.start_time;
        let end_time = ctx.accounts.vesting_account.end_time;
        let recipient = ctx.accounts.vesting_account.recipient;
        let nonce = ctx.accounts.vesting_account.nonce;
        
        require!(!is_revoked, ErrorCode::VestingRevoked);
        require!(!is_paused, ErrorCode::VestingPaused);
        require!(Clock::get()?.unix_timestamp >= cliff_time, ErrorCode::CliffNotReached);
        
        let current_time = Clock::get()?.unix_timestamp;
        let vested_amount = calculate_vested_amount(
            total_amount,
            released_amount,
            start_time,
            end_time,
            current_time,
        )?;
        
        require!(vested_amount > 0, ErrorCode::NothingToClaim);
        
        // Transfer vested tokens to recipient
        let seeds = &[
            b"vesting",
            ctx.accounts.vesting_account.creator.as_ref(),
            recipient.as_ref(),
            &[ctx.bumps.vesting_account],
        ];
        let signer = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.token_account.to_account_info(),
            to: ctx.accounts.recipient_token_account.to_account_info(),
            authority: ctx.accounts.vesting_account.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, vested_amount)?;
        
        // Update state
        let vesting = &mut ctx.accounts.vesting_account;
        vesting.released_amount += vested_amount;
        
        emit!(TokensClaimedEvent {
            vesting_account: vesting.key(),
            recipient,
            amount: vested_amount,
            total_released: vesting.released_amount,
            claimed_at: current_time,
            nonce,
        });
        
        emit!(SecurityEvent {
            event_type: "TOKENS_CLAIMED".to_string(),
            severity: "INFO".to_string(),
            user: recipient,
            timestamp: current_time,
            details: format!("Vested tokens claimed: {}", vested_amount),
            risk_level: 1,
        });
        
        msg!("✅ Claimed {} vested tokens", vested_amount);
        Ok(())
    }

    /// Revoke vesting with enhanced security
    pub fn revoke_vesting(ctx: Context<RevokeVesting>) -> Result<()> {
        // Check if program is paused
        require!(!ctx.accounts.program_state.is_paused, ErrorCode::ProgramPaused);
        
        // Read all values BEFORE mutable borrow
        let is_revocable = ctx.accounts.vesting_account.is_revocable;
        let is_revoked = ctx.accounts.vesting_account.is_revoked;
        let total_amount = ctx.accounts.vesting_account.total_amount;
        let released_amount = ctx.accounts.vesting_account.released_amount;
        let creator = ctx.accounts.vesting_account.creator;
        let recipient = ctx.accounts.vesting_account.recipient;
        let nonce = ctx.accounts.vesting_account.nonce;
        
        require!(is_revocable, ErrorCode::NotRevocable);
        require!(!is_revoked, ErrorCode::AlreadyRevoked);
        
        // Update state
        let vesting = &mut ctx.accounts.vesting_account;
        vesting.is_revoked = true;
        
        // Return unvested tokens to creator
        let remaining = total_amount - released_amount;
        
        let seeds = &[
            b"vesting",
            creator.as_ref(),
            recipient.as_ref(),
            &[ctx.bumps.vesting_account],
        ];
        let signer = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.token_account.to_account_info(),
            to: ctx.accounts.creator_token_account.to_account_info(),
            authority: ctx.accounts.vesting_account.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, remaining)?;
        
        emit!(VestingRevokedEvent {
            vesting_account: vesting.key(),
            revoked_by: creator,
            recipient,
            returned_amount: remaining,
            revoked_at: Clock::get()?.unix_timestamp,
            nonce,
        });
        
        emit!(SecurityEvent {
            event_type: "VESTING_REVOKED".to_string(),
            severity: "WARNING".to_string(),
            user: creator,
            timestamp: Clock::get()?.unix_timestamp,
            details: format!("Vesting revoked: {} tokens returned", remaining),
            risk_level: 3,
        });
        
        msg!("⚠️ Vesting revoked, {} tokens returned", remaining);
        Ok(())
    }

    /// Emergency pause function (multi-sig required)
    pub fn emergency_pause(ctx: Context<EmergencyPause>) -> Result<()> {
        require!(ctx.accounts.program_state.emergency_authority == ctx.accounts.emergency_authority.key(), ErrorCode::Unauthorized);
        require!(!ctx.accounts.program_state.is_paused, ErrorCode::AlreadyPaused);
        
        ctx.accounts.program_state.is_paused = true;
        ctx.accounts.program_state.paused_at = Clock::get()?.unix_timestamp;
        ctx.accounts.program_state.paused_by = ctx.accounts.emergency_authority.key();
        
        emit!(EmergencyPauseEvent {
            paused_by: ctx.accounts.emergency_authority.key(),
            paused_at: Clock::get()?.unix_timestamp,
            reason: "Emergency pause activated".to_string(),
        });
        
        emit!(SecurityEvent {
            event_type: "EMERGENCY_PAUSE".to_string(),
            severity: "CRITICAL".to_string(),
            user: ctx.accounts.emergency_authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
            details: "Program paused by emergency authority".to_string(),
            risk_level: 5,
        });
        
        msg!("🚨 EMERGENCY PAUSE ACTIVATED by {}", ctx.accounts.emergency_authority.key());
        Ok(())
    }

    /// Resume program operations (multi-sig required)
    pub fn emergency_resume(ctx: Context<EmergencyResume>) -> Result<()> {
        require!(ctx.accounts.program_state.emergency_authority == ctx.accounts.emergency_authority.key(), ErrorCode::Unauthorized);
        require!(ctx.accounts.program_state.is_paused, ErrorCode::NotPaused);
        
        ctx.accounts.program_state.is_paused = false;
        ctx.accounts.program_state.resumed_at = Clock::get()?.unix_timestamp;
        ctx.accounts.program_state.resumed_by = ctx.accounts.emergency_authority.key();
        
        emit!(EmergencyResumeEvent {
            resumed_by: ctx.accounts.emergency_authority.key(),
            resumed_at: Clock::get()?.unix_timestamp,
        });
        
        emit!(SecurityEvent {
            event_type: "EMERGENCY_RESUME".to_string(),
            severity: "INFO".to_string(),
            user: ctx.accounts.emergency_authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
            details: "Program resumed by emergency authority".to_string(),
            risk_level: 1,
        });
        
        msg!("✅ PROGRAM RESUMED by {}", ctx.accounts.emergency_authority.key());
        Ok(())
    }

    /// Multi-signature execution for critical operations
    pub fn multisig_execute(ctx: Context<MultiSigExecute>, operation: u8, data: Vec<u8>) -> Result<()> {
        let multisig = &mut ctx.accounts.multisig;
        let signer_index = multisig.signers
            .iter()
            .position(|&signer| signer == ctx.accounts.signer.key())
            .ok_or(ErrorCode::NotAuthorizedSigner)?;
        
        require!(!multisig.executed[signer_index], ErrorCode::AlreadySigned);
        
        multisig.executed[signer_index] = true;
        multisig.signature_count = multisig.signature_count.saturating_add(1);
        
        if multisig.signature_count >= multisig.threshold {
            // Execute the operation
            match operation {
                1 => {
                    // Emergency pause
                    ctx.accounts.program_state.is_paused = true;
                    ctx.accounts.program_state.paused_at = Clock::get()?.unix_timestamp;
                }
                2 => {
                    // Emergency resume
                    ctx.accounts.program_state.is_paused = false;
                    ctx.accounts.program_state.resumed_at = Clock::get()?.unix_timestamp;
                }
                _ => return Err(ErrorCode::InvalidOperation.into()),
            }
            
            // Reset multisig
            multisig.signature_count = 0;
            multisig.executed = vec![false; multisig.signers.len()];
        }
        
        emit!(MultiSigSignedEvent {
            multisig_id: multisig.key(),
            signer: ctx.accounts.signer.key(),
            signature_count: multisig.signature_count,
            threshold: multisig.threshold,
            operation,
        });
        
        msg!("✅ Multi-sig signature recorded: {}/{}", multisig.signature_count, multisig.threshold);
        Ok(())
    }
}

// ========== HELPER FUNCTIONS ==========

fn calculate_vested_amount(
    total_amount: u64,
    released_amount: u64,
    start_time: i64,
    end_time: i64,
    current_time: i64,
) -> Result<u64> {
    if current_time < start_time {
        return Ok(0);
    }
    
    if current_time >= end_time {
        return Ok(total_amount - released_amount);
    }
    
    let elapsed = (current_time - start_time) as u128;
    let duration = (end_time - start_time) as u128;
    
    let vested_total = (total_amount as u128 * elapsed / duration) as u64;
    let claimable = vested_total.saturating_sub(released_amount);
    
    Ok(claimable)
}

fn check_rate_limit(rate_limit: &mut RateLimit, max_actions: u64, window_seconds: i64) -> Result<()> {
    let current_time = Clock::get()?.unix_timestamp;
    
    // Reset window if needed
    if current_time - rate_limit.window_start > window_seconds {
        rate_limit.window_start = current_time;
        rate_limit.action_count = 0;
    }
    
    require!(rate_limit.action_count < max_actions, ErrorCode::RateLimitExceeded);
    
    rate_limit.action_count = rate_limit.action_count.saturating_add(1);
    rate_limit.last_action = current_time;
    
    Ok(())
}

// ========== ACCOUNTS ==========

#[derive(Accounts)]
#[instruction(recipient: Pubkey)]
pub struct CreateVesting<'info> {
    #[account(
        init,
        payer = creator,
        space = 8 + VestingAccount::SPACE,
        seeds = [b"vesting", creator.key().as_ref(), recipient.as_ref()],
        bump
    )]
    pub vesting_account: Account<'info, VestingAccount>,
    
    #[account(
        mut,
        seeds = [b"program_state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(
        init_if_needed,
        payer = creator,
        space = 8 + RateLimit::SPACE,
        seeds = [b"rate_limit", creator.key().as_ref()],
        bump
    )]
    pub rate_limit: Account<'info, RateLimit>,
    
    #[account(mut)]
    pub creator_token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub creator: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClaimVested<'info> {
    #[account(
        mut,
        seeds = [b"vesting", vesting_account.creator.as_ref(), vesting_account.recipient.as_ref()],
        bump,
        has_one = recipient,
    )]
    pub vesting_account: Account<'info, VestingAccount>,
    
    #[account(
        mut,
        seeds = [b"program_state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(
        mut,
        seeds = [b"rate_limit", recipient.key().as_ref()],
        bump
    )]
    pub rate_limit: Account<'info, RateLimit>,
    
    #[account(mut)]
    pub token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub recipient_token_account: Account<'info, TokenAccount>,
    
    pub recipient: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct RevokeVesting<'info> {
    #[account(
        mut,
        seeds = [b"vesting", vesting_account.creator.as_ref(), vesting_account.recipient.as_ref()],
        bump,
        has_one = creator,
    )]
    pub vesting_account: Account<'info, VestingAccount>,
    
    #[account(
        mut,
        seeds = [b"program_state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(mut)]
    pub token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub creator_token_account: Account<'info, TokenAccount>,
    
    pub creator: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct EmergencyPause<'info> {
    #[account(
        mut,
        seeds = [b"program_state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    pub emergency_authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct EmergencyResume<'info> {
    #[account(
        mut,
        seeds = [b"program_state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    pub emergency_authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct MultiSigExecute<'info> {
    #[account(
        mut,
        seeds = [b"multisig"],
        bump
    )]
    pub multisig: Account<'info, MultiSig>,
    
    #[account(
        mut,
        seeds = [b"program_state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    pub signer: Signer<'info>,
}

// ========== STATE ==========

#[account]
pub struct VestingAccount {
    pub creator: Pubkey,                  // 32
    pub recipient: Pubkey,                 // 32
    pub token_account: Pubkey,             // 32
    pub total_amount: u64,                 // 8
    pub released_amount: u64,              // 8
    pub start_time: i64,                   // 8
    pub end_time: i64,                     // 8
    pub cliff_time: i64,                   // 8
    pub release_frequency: i64,            // 8
    pub is_revocable: bool,                // 1
    pub is_revoked: bool,                  // 1
    pub is_paused: bool,                   // 1
    pub created_at: i64,                   // 8
    pub nonce: u64,                        // 8
}

impl VestingAccount {
    pub const SPACE: usize = 32 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 8 + 1 + 1 + 1 + 8 + 8; // 154 bytes
}

#[account]
pub struct ProgramState {
    pub is_paused: bool,                  // 1
    pub emergency_authority: Pubkey,      // 32
    pub paused_at: i64,                   // 8
    pub paused_by: Pubkey,                // 32
    pub resumed_at: i64,                  // 8
    pub resumed_by: Pubkey,               // 32
    pub next_nonce: u64,                  // 8
}

#[account]
pub struct RateLimit {
    pub last_action: i64,                 // 8
    pub action_count: u64,                // 8
    pub window_start: i64,                // 8
}

#[account]
pub struct MultiSig {
    pub threshold: u8,                    // 1
    pub signers: Vec<Pubkey>,             // 4 + (32 * N)
    pub executed: Vec<bool>,              // 4 + (1 * N)
    pub signature_count: u8,              // 1
}

// ========== EVENTS ==========

#[event]
pub struct VestingCreatedEvent {
    pub vesting_account: Pubkey,
    pub creator: Pubkey,
    pub recipient: Pubkey,
    pub total_amount: u64,
    pub start_time: i64,
    pub end_time: i64,
    pub cliff_time: i64,
    pub is_revocable: bool,
    pub created_at: i64,
    pub nonce: u64,
}

#[event]
pub struct TokensClaimedEvent {
    pub vesting_account: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
    pub total_released: u64,
    pub claimed_at: i64,
    pub nonce: u64,
}

#[event]
pub struct VestingRevokedEvent {
    pub vesting_account: Pubkey,
    pub revoked_by: Pubkey,
    pub recipient: Pubkey,
    pub returned_amount: u64,
    pub revoked_at: i64,
    pub nonce: u64,
}

#[event]
pub struct EmergencyPauseEvent {
    pub paused_by: Pubkey,
    pub paused_at: i64,
    pub reason: String,
}

#[event]
pub struct EmergencyResumeEvent {
    pub resumed_by: Pubkey,
    pub resumed_at: i64,
}

#[event]
pub struct MultiSigSignedEvent {
    pub multisig_id: Pubkey,
    pub signer: Pubkey,
    pub signature_count: u8,
    pub threshold: u8,
    pub operation: u8,
}

#[event]
pub struct SecurityEvent {
    pub event_type: String,
    pub severity: String,
    pub user: Pubkey,
    pub timestamp: i64,
    pub details: String,
    pub risk_level: u8,
}

// ========== ERRORS ==========

#[error_code]
pub enum ErrorCode {
    #[msg("Program is paused")]
    ProgramPaused,
    #[msg("Invalid time range")]
    InvalidTimeRange,
    #[msg("Cliff time is before start time")]
    CliffBeforeStart,
    #[msg("Cliff time is after end time")]
    CliffAfterEnd,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Invalid release frequency")]
    InvalidFrequency,
    #[msg("Vesting has been revoked")]
    VestingRevoked,
    #[msg("Vesting is paused")]
    VestingPaused,
    #[msg("Cliff period not reached")]
    CliffNotReached,
    #[msg("Nothing to claim yet")]
    NothingToClaim,
    #[msg("Vesting is not revocable")]
    NotRevocable,
    #[msg("Vesting already revoked")]
    AlreadyRevoked,
    #[msg("Rate limit exceeded")]
    RateLimitExceeded,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Already paused")]
    AlreadyPaused,
    #[msg("Not paused")]
    NotPaused,
    #[msg("Not authorized signer")]
    NotAuthorizedSigner,
    #[msg("Already signed")]
    AlreadySigned,
    #[msg("Invalid operation")]
    InvalidOperation,
}
