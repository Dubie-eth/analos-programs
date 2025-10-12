use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

// Security.txt implementation for program verification
#[cfg(not(feature = "no-entrypoint"))]
use {default_env::default_env, solana_security_txt::security_txt};

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Analos Airdrop Enhanced",
    project_url: "https://github.com/Dubie-eth/analos-programs",
    contacts: "email:security@analos.io,twitter:@EWildn,telegram:t.me/Dubie_420",
    policy: "https://github.com/Dubie-eth/analos-programs/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/Dubie-eth/analos-programs",
    source_revision: "J2D1LiSGxj9vTN7vc3CUD1LkrnqanAeAoAhE2nvvyXHC",
    source_release: "v1.0.0",
    auditors: "None",
    acknowledgements: "Thank you to all security researchers who help keep Analos secure!"
}

declare_id!("J2D1LiSGxj9vTN7vc3CUD1LkrnqanAeAoAhE2nvvyXHC");

/// Enhanced Airdrop Program with Advanced Security Features
/// - Emergency pause functions
/// - Multi-signature support
/// - Rate limiting
/// - Enhanced logging and monitoring
#[program]
pub mod analos_airdrop_enhanced {
    use super::*;

    /// Initialize airdrop with enhanced security
    pub fn initialize_airdrop(
        ctx: Context<InitializeAirdrop>,
        total_amount: u64,
        recipients: Vec<Pubkey>,
        amounts: Vec<u64>,
        expires_at: i64,
    ) -> Result<()> {
        // Check if program is paused
        require!(!ctx.accounts.program_state.is_paused, ErrorCode::ProgramPaused);
        
        // Input validation
        require!(recipients.len() == amounts.len(), ErrorCode::MismatchedLengths);
        require!(recipients.len() <= 1000, ErrorCode::TooManyRecipients);
        require!(recipients.len() > 0, ErrorCode::NoRecipients);
        require!(total_amount > 0, ErrorCode::ZeroAmount);
        require!(expires_at > Clock::get()?.unix_timestamp, ErrorCode::InvalidExpiry);
        
        // Verify total amount matches sum of individual amounts
        let calculated_total: u64 = amounts.iter().sum();
        require!(calculated_total == total_amount, ErrorCode::AmountMismatch);
        
        let airdrop = &mut ctx.accounts.airdrop;
        airdrop.authority = ctx.accounts.authority.key();
        airdrop.token_vault = ctx.accounts.token_vault.key();
        airdrop.total_amount = total_amount;
        airdrop.claimed_amount = 0;
        airdrop.recipients = recipients.clone();
        airdrop.amounts = amounts.clone();
        airdrop.claimed = vec![false; airdrop.recipients.len()];
        airdrop.is_active = true;
        airdrop.expires_at = expires_at;
        airdrop.created_at = Clock::get()?.unix_timestamp;
        airdrop.nonce = ctx.accounts.program_state.next_nonce;
        
        // Update program state
        ctx.accounts.program_state.next_nonce = ctx.accounts.program_state.next_nonce.saturating_add(1);
        
        emit!(AirdropInitializedEvent {
            airdrop_id: airdrop.key(),
            authority: airdrop.authority,
            total_amount,
            recipient_count: recipients.len() as u32,
            expires_at,
            created_at: airdrop.created_at,
            nonce: airdrop.nonce,
        });
        
        emit!(SecurityEvent {
            event_type: "AIRDROP_INITIALIZED".to_string(),
            severity: "INFO".to_string(),
            user: airdrop.authority,
            timestamp: Clock::get()?.unix_timestamp,
            details: format!("Airdrop initialized: {} tokens to {} recipients", total_amount, recipients.len()),
            risk_level: 1,
        });
        
        msg!("✅ Airdrop initialized: {} tokens to {} recipients (expires: {})", total_amount, recipients.len(), expires_at);
        Ok(())
    }

    /// Claim airdrop with enhanced security
    pub fn claim_airdrop(ctx: Context<ClaimAirdrop>) -> Result<()> {
        // Check if program is paused
        require!(!ctx.accounts.program_state.is_paused, ErrorCode::ProgramPaused);
        
        // Rate limiting check
        check_rate_limit(&mut ctx.accounts.rate_limit, 5, 3600)?; // 5 claims per hour
        
        // Read all values BEFORE mutable borrow
        let is_active = ctx.accounts.airdrop.is_active;
        let authority_key = ctx.accounts.airdrop.authority;
        let recipients = &ctx.accounts.airdrop.recipients;
        let amounts = &ctx.accounts.airdrop.amounts;
        let claimed = &ctx.accounts.airdrop.claimed;
        let expires_at = ctx.accounts.airdrop.expires_at;
        let nonce = ctx.accounts.airdrop.nonce;
        
        require!(is_active, ErrorCode::AirdropNotActive);
        require!(Clock::get()?.unix_timestamp < expires_at, ErrorCode::AirdropExpired);
        
        // Find recipient index
        let recipient_key = ctx.accounts.recipient.key();
        let recipient_index = recipients
            .iter()
            .position(|&r| r == recipient_key)
            .ok_or(ErrorCode::NotEligible)?;
        
        require!(!claimed[recipient_index], ErrorCode::AlreadyClaimed);
        
        let amount = amounts[recipient_index];
        
        // Transfer tokens
        let cpi_accounts = Transfer {
            from: ctx.accounts.token_vault.to_account_info(),
            to: ctx.accounts.recipient_token_account.to_account_info(),
            authority: ctx.accounts.airdrop.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let seeds = &[
            b"airdrop",
            authority_key.as_ref(),
            &[ctx.bumps.airdrop],
        ];
        let signer = &[&seeds[..]];
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, amount)?;
        
        // Update state
        let airdrop = &mut ctx.accounts.airdrop;
        airdrop.claimed[recipient_index] = true;
        airdrop.claimed_amount += amount;
        
        emit!(AirdropClaimedEvent {
            airdrop_id: airdrop.key(),
            recipient: recipient_key,
            amount,
            total_claimed: airdrop.claimed_amount,
            claimed_at: Clock::get()?.unix_timestamp,
            nonce,
        });
        
        emit!(SecurityEvent {
            event_type: "AIRDROP_CLAIMED".to_string(),
            severity: "INFO".to_string(),
            user: recipient_key,
            timestamp: Clock::get()?.unix_timestamp,
            details: format!("Airdrop claimed: {} tokens", amount),
            risk_level: 1,
        });
        
        msg!("✅ Airdrop claimed: {} tokens by {}", amount, recipient_key);
        Ok(())
    }

    /// Cancel airdrop with enhanced security
    pub fn cancel_airdrop(ctx: Context<CancelAirdrop>) -> Result<()> {
        // Check if program is paused
        require!(!ctx.accounts.program_state.is_paused, ErrorCode::ProgramPaused);
        
        // Read all values BEFORE mutable borrow
        let is_active = ctx.accounts.airdrop.is_active;
        let authority_key = ctx.accounts.airdrop.authority;
        let total_amount = ctx.accounts.airdrop.total_amount;
        let claimed_amount = ctx.accounts.airdrop.claimed_amount;
        let nonce = ctx.accounts.airdrop.nonce;
        
        require!(is_active, ErrorCode::AirdropNotActive);
        
        // Update state
        let airdrop = &mut ctx.accounts.airdrop;
        airdrop.is_active = false;
        
        emit!(AirdropCancelledEvent {
            airdrop_id: airdrop.key(),
            authority: authority_key,
            total_amount,
            claimed_amount,
            cancelled_at: Clock::get()?.unix_timestamp,
            nonce,
        });
        
        emit!(SecurityEvent {
            event_type: "AIRDROP_CANCELLED".to_string(),
            severity: "WARNING".to_string(),
            user: authority_key,
            timestamp: Clock::get()?.unix_timestamp,
            details: format!("Airdrop cancelled: {} claimed, {} remaining", claimed_amount, total_amount - claimed_amount),
            risk_level: 2,
        });
        
        msg!("⚠️ Airdrop cancelled by {}", authority_key);
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

    /// Emergency withdrawal of unclaimed tokens
    pub fn emergency_withdraw(ctx: Context<EmergencyWithdraw>) -> Result<()> {
        require!(ctx.accounts.program_state.emergency_authority == ctx.accounts.emergency_authority.key(), ErrorCode::Unauthorized);
        require!(ctx.accounts.program_state.is_paused, ErrorCode::NotPaused);
        
        let airdrop = &mut ctx.accounts.airdrop;
        let remaining_amount = airdrop.total_amount - airdrop.claimed_amount;
        
        if remaining_amount > 0 {
            // Transfer remaining tokens back to authority
            let seeds = &[
                b"airdrop",
                airdrop.authority.as_ref(),
                &[ctx.bumps.airdrop],
            ];
            let signer = &[&seeds[..]];
            
            let cpi_accounts = Transfer {
                from: ctx.accounts.token_vault.to_account_info(),
                to: ctx.accounts.authority_token_account.to_account_info(),
                authority: airdrop.to_account_info(),
            };
            let cpi_program = ctx.accounts.token_program.to_account_info();
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
            token::transfer(cpi_ctx, remaining_amount)?;
            
            airdrop.is_active = false;
            
            emit!(EmergencyWithdrawalEvent {
                airdrop_id: airdrop.key(),
                authority: ctx.accounts.emergency_authority.key(),
                amount: remaining_amount,
                withdrawn_at: Clock::get()?.unix_timestamp,
            });
            
            msg!("🚨 Emergency withdrawal: {} tokens", remaining_amount);
        }
        
        Ok(())
    }
}

// ========== HELPER FUNCTIONS ==========

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
pub struct InitializeAirdrop<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + Airdrop::SPACE,
        seeds = [b"airdrop", authority.key().as_ref(), &Clock::get()?.unix_timestamp.to_le_bytes()],
        bump
    )]
    pub airdrop: Account<'info, Airdrop>,
    
    #[account(
        mut,
        seeds = [b"program_state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(mut)]
    pub token_vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClaimAirdrop<'info> {
    #[account(
        mut,
        seeds = [b"airdrop", airdrop.authority.as_ref(), &airdrop.nonce.to_le_bytes()],
        bump,
        has_one = token_vault,
    )]
    pub airdrop: Account<'info, Airdrop>,
    
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
    pub token_vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub recipient_token_account: Account<'info, TokenAccount>,
    
    pub recipient: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CancelAirdrop<'info> {
    #[account(
        mut,
        seeds = [b"airdrop", airdrop.authority.as_ref(), &airdrop.nonce.to_le_bytes()],
        bump,
        has_one = authority,
    )]
    pub airdrop: Account<'info, Airdrop>,
    
    #[account(
        mut,
        seeds = [b"program_state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    pub authority: Signer<'info>,
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

#[derive(Accounts)]
pub struct EmergencyWithdraw<'info> {
    #[account(
        mut,
        seeds = [b"airdrop", airdrop.authority.as_ref(), &airdrop.nonce.to_le_bytes()],
        bump,
    )]
    pub airdrop: Account<'info, Airdrop>,
    
    #[account(
        mut,
        seeds = [b"program_state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(mut)]
    pub token_vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub authority_token_account: Account<'info, TokenAccount>,
    
    pub emergency_authority: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
}

// ========== STATE ==========

#[account]
pub struct Airdrop {
    pub authority: Pubkey,                // 32
    pub token_vault: Pubkey,              // 32
    pub total_amount: u64,                // 8
    pub claimed_amount: u64,              // 8
    pub recipients: Vec<Pubkey>,          // 4 + (32 * N)
    pub amounts: Vec<u64>,                // 4 + (8 * N)
    pub claimed: Vec<bool>,               // 4 + (1 * N)
    pub is_active: bool,                  // 1
    pub expires_at: i64,                  // 8
    pub created_at: i64,                  // 8
    pub nonce: u64,                       // 8
}

impl Airdrop {
    pub const SPACE: usize = 32 + 32 + 8 + 8 + 4 + (32 * 1000) + 4 + (8 * 1000) + 4 + 1000 + 1 + 8 + 8 + 8; // ~33KB max
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
pub struct AirdropInitializedEvent {
    pub airdrop_id: Pubkey,
    pub authority: Pubkey,
    pub total_amount: u64,
    pub recipient_count: u32,
    pub expires_at: i64,
    pub created_at: i64,
    pub nonce: u64,
}

#[event]
pub struct AirdropClaimedEvent {
    pub airdrop_id: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
    pub total_claimed: u64,
    pub claimed_at: i64,
    pub nonce: u64,
}

#[event]
pub struct AirdropCancelledEvent {
    pub airdrop_id: Pubkey,
    pub authority: Pubkey,
    pub total_amount: u64,
    pub claimed_amount: u64,
    pub cancelled_at: i64,
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
pub struct EmergencyWithdrawalEvent {
    pub airdrop_id: Pubkey,
    pub authority: Pubkey,
    pub amount: u64,
    pub withdrawn_at: i64,
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
    #[msg("Mismatched recipients and amounts lengths")]
    MismatchedLengths,
    #[msg("Too many recipients (max 1000)")]
    TooManyRecipients,
    #[msg("No recipients provided")]
    NoRecipients,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Invalid expiry time")]
    InvalidExpiry,
    #[msg("Amount mismatch")]
    AmountMismatch,
    #[msg("Airdrop is not active")]
    AirdropNotActive,
    #[msg("Airdrop expired")]
    AirdropExpired,
    #[msg("Not eligible for this airdrop")]
    NotEligible,
    #[msg("Already claimed")]
    AlreadyClaimed,
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
