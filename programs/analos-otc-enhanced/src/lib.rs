use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

// Security.txt implementation for program verification
#[cfg(not(feature = "no-entrypoint"))]
use {default_env::default_env, solana_security_txt::security_txt};

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Analos OTC Enhanced",
    project_url: "https://github.com/Dubie-eth/analos-programs",
    contacts: "email:support@launchonlos.fun,twitter:@EWildn,telegram:t.me/Dubie_420",
    policy: "https://github.com/Dubie-eth/analos-programs/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/Dubie-eth/analos-programs",
    source_revision: "7hnWVgRxu2dNWiNAzNB2jWoubzMcdY6HNysjhLiawXPY",
    source_release: "v1.0.0",
    auditors: "None",
    acknowledgements: "Thank you to all security researchers who help keep Analos secure!"
}

declare_id!("7hnWVgRxu2dNWiNAzNB2jWoubzMcdY6HNysjhLiawXPY");

/// Enhanced OTC Marketplace Program with Advanced Security Features
/// - Emergency pause functions
/// - Multi-signature support
/// - Rate limiting
/// - Enhanced logging and monitoring
#[program]
pub mod analos_otc_enhanced {
    use super::*;

    /// Create an OTC offer with enhanced security
    pub fn create_offer(
        ctx: Context<CreateOffer>,
        offer_amount: u64,
        request_amount: u64,
        request_token: Pubkey,
        expires_at: i64,
    ) -> Result<()> {
        // Check if program is paused
        require!(!ctx.accounts.program_state.is_paused, ErrorCode::ProgramPaused);
        
        // Rate limiting check
        check_rate_limit(&mut ctx.accounts.rate_limit, 10, 3600)?; // 10 offers per hour
        
        // Input validation
        require!(offer_amount > 0, ErrorCode::ZeroAmount);
        require!(request_amount > 0, ErrorCode::ZeroAmount);
        require!(expires_at > Clock::get()?.unix_timestamp, ErrorCode::InvalidExpiry);
        
        let offer = &mut ctx.accounts.offer;
        offer.maker = ctx.accounts.maker.key();
        offer.offer_token_account = ctx.accounts.offer_token_account.key();
        offer.offer_amount = offer_amount;
        offer.request_amount = request_amount;
        offer.request_token = request_token;
        offer.expires_at = expires_at;
        offer.is_active = true;
        offer.taker = None;
        offer.created_at = Clock::get()?.unix_timestamp;
        offer.nonce = ctx.accounts.program_state.next_nonce;
        
        // Update program state
        ctx.accounts.program_state.next_nonce = ctx.accounts.program_state.next_nonce.saturating_add(1);
        
        // Transfer tokens to escrow
        let cpi_accounts = Transfer {
            from: ctx.accounts.maker_token_account.to_account_info(),
            to: ctx.accounts.offer_token_account.to_account_info(),
            authority: ctx.accounts.maker.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, offer_amount)?;
        
        emit!(OfferCreatedEvent {
            offer_id: offer.key(),
            maker: offer.maker,
            offer_amount,
            request_amount,
            request_token,
            expires_at,
            created_at: offer.created_at,
            nonce: offer.nonce,
        });
        
        emit!(SecurityEvent {
            event_type: "OFFER_CREATED".to_string(),
            severity: "INFO".to_string(),
            user: offer.maker,
            timestamp: Clock::get()?.unix_timestamp,
            details: format!("Offer created: {} for {}", offer_amount, request_amount),
            risk_level: 1,
        });
        
        msg!("✅ OTC offer created: {} for {} (expires: {})", offer_amount, request_amount, expires_at);
        Ok(())
    }

    /// Accept an OTC offer with enhanced security
    pub fn accept_offer(ctx: Context<AcceptOffer>) -> Result<()> {
        // Check if program is paused
        require!(!ctx.accounts.program_state.is_paused, ErrorCode::ProgramPaused);
        
        // Rate limiting check
        check_rate_limit(&mut ctx.accounts.rate_limit, 20, 3600)?; // 20 accepts per hour
        
        // Read all values BEFORE mutable borrow
        let is_active = ctx.accounts.offer.is_active;
        let taker_option = ctx.accounts.offer.taker;
        let maker_key = ctx.accounts.offer.maker;
        let request_amount = ctx.accounts.offer.request_amount;
        let offer_amount = ctx.accounts.offer.offer_amount;
        let expires_at = ctx.accounts.offer.expires_at;
        let nonce = ctx.accounts.offer.nonce;
        
        require!(is_active, ErrorCode::OfferNotActive);
        require!(taker_option.is_none(), ErrorCode::OfferAlreadyTaken);
        require!(Clock::get()?.unix_timestamp < expires_at, ErrorCode::OfferExpired);
        
        // Transfer requested tokens from taker to maker
        let cpi_accounts = Transfer {
            from: ctx.accounts.taker_request_token_account.to_account_info(),
            to: ctx.accounts.maker_request_token_account.to_account_info(),
            authority: ctx.accounts.taker.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, request_amount)?;
        
        // Transfer offered tokens from escrow to taker
        let seeds = &[
            b"offer",
            maker_key.as_ref(),
            &[ctx.bumps.offer],
        ];
        let signer = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.offer_token_account.to_account_info(),
            to: ctx.accounts.taker_offer_token_account.to_account_info(),
            authority: ctx.accounts.offer.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, offer_amount)?;
        
        // Update state
        let offer = &mut ctx.accounts.offer;
        offer.taker = Some(ctx.accounts.taker.key());
        offer.is_active = false;
        
        emit!(OfferAcceptedEvent {
            offer_id: offer.key(),
            maker: maker_key,
            taker: ctx.accounts.taker.key(),
            offer_amount,
            request_amount,
            accepted_at: Clock::get()?.unix_timestamp,
            nonce,
        });
        
        emit!(SecurityEvent {
            event_type: "OFFER_ACCEPTED".to_string(),
            severity: "INFO".to_string(),
            user: ctx.accounts.taker.key(),
            timestamp: Clock::get()?.unix_timestamp,
            details: format!("Offer accepted: {} for {}", offer_amount, request_amount),
            risk_level: 2,
        });
        
        msg!("✅ OTC offer accepted by {}", ctx.accounts.taker.key());
        Ok(())
    }

    /// Cancel an OTC offer with enhanced security
    pub fn cancel_offer(ctx: Context<CancelOffer>) -> Result<()> {
        // Check if program is paused
        require!(!ctx.accounts.program_state.is_paused, ErrorCode::ProgramPaused);
        
        // Read all values BEFORE mutable borrow
        let is_active = ctx.accounts.offer.is_active;
        let taker_option = ctx.accounts.offer.taker;
        let maker_key = ctx.accounts.offer.maker;
        let offer_amount = ctx.accounts.offer.offer_amount;
        let nonce = ctx.accounts.offer.nonce;
        
        require!(is_active, ErrorCode::OfferNotActive);
        require!(taker_option.is_none(), ErrorCode::OfferAlreadyTaken);
        
        // Return tokens to maker
        let seeds = &[
            b"offer",
            maker_key.as_ref(),
            &[ctx.bumps.offer],
        ];
        let signer = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.offer_token_account.to_account_info(),
            to: ctx.accounts.maker_token_account.to_account_info(),
            authority: ctx.accounts.offer.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, offer_amount)?;
        
        // Update state
        let offer = &mut ctx.accounts.offer;
        offer.is_active = false;
        
        emit!(OfferCancelledEvent {
            offer_id: offer.key(),
            maker: maker_key,
            offer_amount,
            cancelled_at: Clock::get()?.unix_timestamp,
            nonce,
        });
        
        emit!(SecurityEvent {
            event_type: "OFFER_CANCELLED".to_string(),
            severity: "INFO".to_string(),
            user: maker_key,
            timestamp: Clock::get()?.unix_timestamp,
            details: format!("Offer cancelled: {} tokens returned", offer_amount),
            risk_level: 1,
        });
        
        msg!("✅ OTC offer cancelled by {}", maker_key);
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
pub struct CreateOffer<'info> {
    #[account(
        init,
        payer = maker,
        space = 8 + Offer::SPACE,
        seeds = [b"offer", maker.key().as_ref(), &Clock::get()?.unix_timestamp.to_le_bytes()],
        bump
    )]
    pub offer: Account<'info, Offer>,
    
    #[account(
        mut,
        seeds = [b"program_state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(
        init_if_needed,
        payer = maker,
        space = 8 + RateLimit::SPACE,
        seeds = [b"rate_limit", maker.key().as_ref()],
        bump
    )]
    pub rate_limit: Account<'info, RateLimit>,
    
    #[account(mut)]
    pub offer_token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub maker_token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub maker: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AcceptOffer<'info> {
    #[account(
        mut,
        seeds = [b"offer", offer.maker.as_ref(), &offer.nonce.to_le_bytes()],
        bump,
        has_one = offer_token_account,
    )]
    pub offer: Account<'info, Offer>,
    
    #[account(
        mut,
        seeds = [b"program_state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(
        mut,
        seeds = [b"rate_limit", taker.key().as_ref()],
        bump
    )]
    pub rate_limit: Account<'info, RateLimit>,
    
    #[account(mut)]
    pub offer_token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub maker_request_token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub taker_request_token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub taker_offer_token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub taker: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CancelOffer<'info> {
    #[account(
        mut,
        seeds = [b"offer", offer.maker.as_ref(), &offer.nonce.to_le_bytes()],
        bump,
        has_one = maker,
        has_one = offer_token_account,
    )]
    pub offer: Account<'info, Offer>,
    
    #[account(
        mut,
        seeds = [b"program_state"],
        bump
    )]
    pub program_state: Account<'info, ProgramState>,
    
    #[account(mut)]
    pub offer_token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub maker_token_account: Account<'info, TokenAccount>,
    
    pub maker: Signer<'info>,
    
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
pub struct Offer {
    pub maker: Pubkey,                    // 32
    pub offer_token_account: Pubkey,      // 32
    pub offer_amount: u64,                // 8
    pub request_amount: u64,              // 8
    pub request_token: Pubkey,            // 32
    pub expires_at: i64,                  // 8
    pub is_active: bool,                  // 1
    pub taker: Option<Pubkey>,            // 33
    pub created_at: i64,                  // 8
    pub nonce: u64,                       // 8
}

impl Offer {
    pub const SPACE: usize = 32 + 32 + 8 + 8 + 32 + 8 + 1 + 33 + 8 + 8; // 162 bytes
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

impl RateLimit {
    pub const SPACE: usize = 8 + 8 + 8; // 24 bytes
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
pub struct OfferCreatedEvent {
    pub offer_id: Pubkey,
    pub maker: Pubkey,
    pub offer_amount: u64,
    pub request_amount: u64,
    pub request_token: Pubkey,
    pub expires_at: i64,
    pub created_at: i64,
    pub nonce: u64,
}

#[event]
pub struct OfferAcceptedEvent {
    pub offer_id: Pubkey,
    pub maker: Pubkey,
    pub taker: Pubkey,
    pub offer_amount: u64,
    pub request_amount: u64,
    pub accepted_at: i64,
    pub nonce: u64,
}

#[event]
pub struct OfferCancelledEvent {
    pub offer_id: Pubkey,
    pub maker: Pubkey,
    pub offer_amount: u64,
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
    #[msg("Offer is not active")]
    OfferNotActive,
    #[msg("Offer already taken")]
    OfferAlreadyTaken,
    #[msg("Offer expired")]
    OfferExpired,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Invalid expiry time")]
    InvalidExpiry,
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
