use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

declare_id!("VEST11111111111111111111111111111111111111");

/// Token Vesting Program - Streamflow-inspired
/// Lock tokens with customizable release schedules
#[program]
pub mod analos_vesting {
    use super::*;

    /// Create a vesting schedule
    pub fn create_vesting(
        ctx: Context<CreateVesting>,
        total_amount: u64,
        start_time: i64,
        end_time: i64,
        cliff_time: i64,
        release_frequency: i64, // seconds between releases
        recipient: Pubkey,
    ) -> Result<()> {
        let vesting = &mut ctx.accounts.vesting_account;
        
        require!(end_time > start_time, ErrorCode::InvalidTimeRange);
        require!(cliff_time >= start_time, ErrorCode::CliffBeforeStart);
        require!(cliff_time <= end_time, ErrorCode::CliffAfterEnd);
        require!(total_amount > 0, ErrorCode::ZeroAmount);
        require!(release_frequency > 0, ErrorCode::InvalidFrequency);
        
        vesting.creator = ctx.accounts.creator.key();
        vesting.recipient = recipient;
        vesting.token_account = ctx.accounts.token_account.key();
        vesting.total_amount = total_amount;
        vesting.released_amount = 0;
        vesting.start_time = start_time;
        vesting.end_time = end_time;
        vesting.cliff_time = cliff_time;
        vesting.release_frequency = release_frequency;
        vesting.is_revocable = true;
        vesting.is_revoked = false;
        
        // Transfer tokens to vesting account
        let cpi_accounts = Transfer {
            from: ctx.accounts.creator_token_account.to_account_info(),
            to: ctx.accounts.token_account.to_account_info(),
            authority: ctx.accounts.creator.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, total_amount)?;
        
        msg!("✅ Vesting created: {} tokens over {} seconds", total_amount, end_time - start_time);
        
        emit!(VestingCreatedEvent {
            vesting_account: vesting.key(),
            recipient,
            total_amount,
            start_time,
            end_time,
        });
        
        Ok(())
    }

    /// Claim vested tokens
    pub fn claim_vested(
        ctx: Context<ClaimVested>,
    ) -> Result<()> {
        let vesting = &mut ctx.accounts.vesting_account;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;
        
        require!(!vesting.is_revoked, ErrorCode::VestingRevoked);
        require!(current_time >= vesting.cliff_time, ErrorCode::CliffNotReached);
        
        let vested_amount = calculate_vested_amount(
            vesting.total_amount,
            vesting.released_amount,
            vesting.start_time,
            vesting.end_time,
            current_time,
        )?;
        
        require!(vested_amount > 0, ErrorCode::NothingToClaim);
        
        // Transfer vested tokens to recipient
        let seeds = &[
            b"vesting",
            vesting.creator.as_ref(),
            vesting.recipient.as_ref(),
            &[ctx.bumps.vesting_account],
        ];
        let signer = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.token_account.to_account_info(),
            to: ctx.accounts.recipient_token_account.to_account_info(),
            authority: vesting.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, vested_amount)?;
        
        vesting.released_amount += vested_amount;
        
        msg!("✅ Claimed {} vested tokens", vested_amount);
        
        emit!(TokensClaimedEvent {
            vesting_account: vesting.key(),
            recipient: vesting.recipient,
            amount: vested_amount,
            total_released: vesting.released_amount,
        });
        
        Ok(())
    }

    /// Revoke vesting (if revocable)
    pub fn revoke_vesting(
        ctx: Context<RevokeVesting>,
    ) -> Result<()> {
        let vesting = &mut ctx.accounts.vesting_account;
        
        require!(vesting.is_revocable, ErrorCode::NotRevocable);
        require!(!vesting.is_revoked, ErrorCode::AlreadyRevoked);
        
        vesting.is_revoked = true;
        
        // Return unvested tokens to creator
        let remaining = vesting.total_amount - vesting.released_amount;
        
        let seeds = &[
            b"vesting",
            vesting.creator.as_ref(),
            vesting.recipient.as_ref(),
            &[ctx.bumps.vesting_account],
        ];
        let signer = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.token_account.to_account_info(),
            to: ctx.accounts.creator_token_account.to_account_info(),
            authority: vesting.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, remaining)?;
        
        msg!("✅ Vesting revoked, {} tokens returned", remaining);
        
        emit!(VestingRevokedEvent {
            vesting_account: vesting.key(),
            revoked_by: ctx.accounts.creator.key(),
            returned_amount: remaining,
        });
        
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
    
    #[account(mut)]
    pub token_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub creator_token_account: Account<'info, TokenAccount>,
    
    pub creator: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
}

// ========== STATE ==========

#[account]
pub struct VestingAccount {
    pub creator: Pubkey,           // 32
    pub recipient: Pubkey,         // 32
    pub token_account: Pubkey,     // 32
    pub total_amount: u64,         // 8
    pub released_amount: u64,      // 8
    pub start_time: i64,           // 8
    pub end_time: i64,             // 8
    pub cliff_time: i64,           // 8
    pub release_frequency: i64,    // 8
    pub is_revocable: bool,        // 1
    pub is_revoked: bool,          // 1
}

impl VestingAccount {
    pub const SPACE: usize = 32 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 8 + 1 + 1; // 146 bytes
}

// ========== EVENTS ==========

#[event]
pub struct VestingCreatedEvent {
    pub vesting_account: Pubkey,
    pub recipient: Pubkey,
    pub total_amount: u64,
    pub start_time: i64,
    pub end_time: i64,
}

#[event]
pub struct TokensClaimedEvent {
    pub vesting_account: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
    pub total_released: u64,
}

#[event]
pub struct VestingRevokedEvent {
    pub vesting_account: Pubkey,
    pub revoked_by: Pubkey,
    pub returned_amount: u64,
}

// ========== ERRORS ==========

#[error_code]
pub enum ErrorCode {
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
    #[msg("Cliff period not reached")]
    CliffNotReached,
    #[msg("Nothing to claim yet")]
    NothingToClaim,
    #[msg("Vesting is not revocable")]
    NotRevocable,
    #[msg("Vesting already revoked")]
    AlreadyRevoked,
}

