use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

declare_id!("LOCK11111111111111111111111111111111111111");

/// Token Lock Program - Prove liquidity/supply is locked
/// Inspired by Streamflow Token Lock
#[program]
pub mod analos_token_lock {
    use super::*;

    /// Create a token lock
    pub fn create_lock(
        ctx: Context<CreateLock>,
        amount: u64,
        unlock_time: i64,
        is_extendable: bool,
        label: String,
    ) -> Result<()> {
        let lock = &mut ctx.accounts.lock_account;
        let current_time = Clock::get()?.unix_timestamp;
        
        require!(unlock_time > current_time, ErrorCode::InvalidUnlockTime);
        require!(amount > 0, ErrorCode::ZeroAmount);
        require!(label.len() <= 32, ErrorCode::LabelTooLong);
        
        lock.owner = ctx.accounts.owner.key();
        lock.token_account = ctx.accounts.token_account.key();
        lock.token_mint = ctx.accounts.token_account.mint;
        lock.amount = amount;
        lock.unlock_time = unlock_time;
        lock.created_at = current_time;
        lock.is_extendable = is_extendable;
        lock.is_unlocked = false;
        lock.label = label.clone();
        
        // Transfer tokens to lock
        let cpi_accounts = Transfer {
            from: ctx.accounts.owner_token_account.to_account_info(),
            to: ctx.accounts.token_account.to_account_info(),
            authority: ctx.accounts.owner.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_ctx, amount)?;
        
        msg!("🔒 Locked {} tokens until {} ({})", amount, unlock_time, label);
        
        emit!(LockCreatedEvent {
            lock_account: lock.key(),
            owner: lock.owner,
            amount,
            unlock_time,
            label,
        });
        
        Ok(())
    }

    /// Extend lock period (if extendable)
    pub fn extend_lock(
        ctx: Context<ExtendLock>,
        new_unlock_time: i64,
    ) -> Result<()> {
        let lock = &mut ctx.accounts.lock_account;
        
        require!(lock.is_extendable, ErrorCode::NotExtendable);
        require!(!lock.is_unlocked, ErrorCode::AlreadyUnlocked);
        require!(new_unlock_time > lock.unlock_time, ErrorCode::InvalidExtension);
        
        let old_unlock_time = lock.unlock_time;
        lock.unlock_time = new_unlock_time;
        
        msg!("⏰ Extended lock from {} to {}", old_unlock_time, new_unlock_time);
        
        emit!(LockExtendedEvent {
            lock_account: lock.key(),
            old_unlock_time,
            new_unlock_time,
        });
        
        Ok(())
    }

    /// Unlock tokens (after unlock time)
    pub fn unlock_tokens(
        ctx: Context<UnlockTokens>,
    ) -> Result<()> {
        let lock = &mut ctx.accounts.lock_account;
        let current_time = Clock::get()?.unix_timestamp;
        
        require!(current_time >= lock.unlock_time, ErrorCode::StillLocked);
        require!(!lock.is_unlocked, ErrorCode::AlreadyUnlocked);
        
        lock.is_unlocked = true;
        
        // Transfer tokens back to owner
        let seeds = &[
            b"lock",
            lock.owner.as_ref(),
            lock.token_mint.as_ref(),
            &[ctx.bumps.lock_account],
        ];
        let signer = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.token_account.to_account_info(),
            to: ctx.accounts.owner_token_account.to_account_info(),
            authority: lock.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer
        );
        token::transfer(cpi_ctx, lock.amount)?;
        
        msg!("🔓 Unlocked {} tokens", lock.amount);
        
        emit!(TokensUnlockedEvent {
            lock_account: lock.key(),
            owner: lock.owner,
            amount: lock.amount,
        });
        
        Ok(())
    }

    /// Emergency unlock (owner only, burns some tokens as penalty)
    pub fn emergency_unlock(
        ctx: Context<EmergencyUnlock>,
        penalty_bps: u16,
    ) -> Result<()> {
        let lock = &mut ctx.accounts.lock_account;
        
        require!(!lock.is_unlocked, ErrorCode::AlreadyUnlocked);
        require!(penalty_bps <= 5000, ErrorCode::PenaltyTooHigh); // Max 50% penalty
        
        let penalty_amount = (lock.amount as u128 * penalty_bps as u128 / 10000) as u64;
        let return_amount = lock.amount - penalty_amount;
        
        lock.is_unlocked = true;
        
        // Transfer tokens back minus penalty
        let seeds = &[
            b"lock",
            lock.owner.as_ref(),
            lock.token_mint.as_ref(),
            &[ctx.bumps.lock_account],
        ];
        let signer = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.token_account.to_account_info(),
            to: ctx.accounts.owner_token_account.to_account_info(),
            authority: lock.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer
        );
        token::transfer(cpi_ctx, return_amount)?;
        
        msg!("⚠️ Emergency unlock: {} returned, {} penalty", return_amount, penalty_amount);
        
        emit!(EmergencyUnlockEvent {
            lock_account: lock.key(),
            returned_amount: return_amount,
            penalty_amount,
        });
        
        Ok(())
    }
}

// ========== ACCOUNTS ==========

#[derive(Accounts)]
#[instruction(token_mint: Pubkey)]
pub struct CreateLock<'info> {
    #[account(
        init,
        payer = owner,
        space = 8 + LockAccount::SPACE,
        seeds = [b"lock", owner.key().as_ref(), token_mint.as_ref()],
        bump
    )]
    pub lock_account: Account<'info, LockAccount>,
    
    #[account(mut)]
    pub owner_token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub owner: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ExtendLock<'info> {
    #[account(
        mut,
        seeds = [b"lock", lock_account.owner.as_ref(), lock_account.token_mint.as_ref()],
        bump,
        has_one = owner,
    )]
    pub lock_account: Account<'info, LockAccount>,
    
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct UnlockTokens<'info> {
    #[account(
        mut,
        seeds = [b"lock", lock_account.owner.as_ref(), lock_account.token_mint.as_ref()],
        bump,
        has_one = owner,
    )]
    pub lock_account: Account<'info, LockAccount>,
    
    #[account(mut)]
    pub token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub owner_token_account: Account<'info, TokenAccount>,
    
    pub owner: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct EmergencyUnlock<'info> {
    #[account(
        mut,
        seeds = [b"lock", lock_account.owner.as_ref(), lock_account.token_mint.as_ref()],
        bump,
        has_one = owner,
    )]
    pub lock_account: Account<'info, LockAccount>,
    
    #[account(mut)]
    pub token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub owner_token_account: Account<'info, TokenAccount>,
    
    pub owner: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
}

// ========== STATE ==========

#[account]
pub struct LockAccount {
    pub owner: Pubkey,              // 32
    pub token_account: Pubkey,      // 32
    pub token_mint: Pubkey,         // 32
    pub amount: u64,                // 8
    pub unlock_time: i64,           // 8
    pub created_at: i64,            // 8
    pub is_extendable: bool,        // 1
    pub is_unlocked: bool,          // 1
    pub label: String,              // 4 + 32
}

impl LockAccount {
    pub const SPACE: usize = 32 + 32 + 32 + 8 + 8 + 8 + 1 + 1 + 4 + 32; // 158 bytes
}

// ========== EVENTS ==========

#[event]
pub struct LockCreatedEvent {
    pub lock_account: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,
    pub unlock_time: i64,
    pub label: String,
}

#[event]
pub struct LockExtendedEvent {
    pub lock_account: Pubkey,
    pub old_unlock_time: i64,
    pub new_unlock_time: i64,
}

#[event]
pub struct TokensUnlockedEvent {
    pub lock_account: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,
}

#[event]
pub struct EmergencyUnlockEvent {
    pub lock_account: Pubkey,
    pub returned_amount: u64,
    pub penalty_amount: u64,
}

// ========== ERRORS ==========

#[error_code]
pub enum ErrorCode {
    #[msg("Unlock time must be in the future")]
    InvalidUnlockTime,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Label too long (max 32 characters)")]
    LabelTooLong,
    #[msg("Lock is not extendable")]
    NotExtendable,
    #[msg("Tokens already unlocked")]
    AlreadyUnlocked,
    #[msg("New unlock time must be later than current")]
    InvalidExtension,
    #[msg("Tokens still locked")]
    StillLocked,
    #[msg("Penalty too high (max 50%)")]
    PenaltyTooHigh,
}

