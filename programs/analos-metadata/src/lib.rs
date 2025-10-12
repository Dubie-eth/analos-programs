use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token};

declare_id!("META11111111111111111111111111111111111111");

/// Lightweight NFT Metadata Program for Analos
/// Compatible with SPL tokens and your NFT Launchpad
#[program]
pub mod analos_metadata {
    use super::*;

    /// Create metadata for an NFT
    pub fn create_metadata(
        ctx: Context<CreateMetadata>,
        name: String,
        symbol: String,
        uri: String,
    ) -> Result<()> {
        let metadata = &mut ctx.accounts.metadata;
        
        require!(name.len() <= 32, ErrorCode::NameTooLong);
        require!(symbol.len() <= 10, ErrorCode::SymbolTooLong);
        require!(uri.len() <= 200, ErrorCode::UriTooLong);
        
        metadata.mint = ctx.accounts.mint.key();
        metadata.update_authority = ctx.accounts.update_authority.key();
        metadata.name = name;
        metadata.symbol = symbol;
        metadata.uri = uri;
        metadata.is_mutable = true;
        
        msg!("✅ NFT Metadata created for mint: {}", ctx.accounts.mint.key());
        
        emit!(MetadataCreatedEvent {
            mint: ctx.accounts.mint.key(),
            metadata: metadata.key(),
            name: metadata.name.clone(),
            symbol: metadata.symbol.clone(),
            uri: metadata.uri.clone(),
        });
        
        Ok(())
    }

    /// Update metadata (only by update authority)
    pub fn update_metadata(
        ctx: Context<UpdateMetadata>,
        name: Option<String>,
        symbol: Option<String>,
        uri: Option<String>,
    ) -> Result<()> {
        let metadata = &mut ctx.accounts.metadata;
        
        if let Some(new_name) = name {
            require!(new_name.len() <= 32, ErrorCode::NameTooLong);
            metadata.name = new_name;
        }
        
        if let Some(new_symbol) = symbol {
            require!(new_symbol.len() <= 10, ErrorCode::SymbolTooLong);
            metadata.symbol = new_symbol;
        }
        
        if let Some(new_uri) = uri {
            require!(new_uri.len() <= 200, ErrorCode::UriTooLong);
            metadata.uri = new_uri;
        }
        
        msg!("✅ NFT Metadata updated for mint: {}", metadata.mint);
        
        emit!(MetadataUpdatedEvent {
            mint: metadata.mint,
            metadata: metadata.key(),
            name: metadata.name.clone(),
            symbol: metadata.symbol.clone(),
            uri: metadata.uri.clone(),
        });
        
        Ok(())
    }

    /// Transfer update authority
    pub fn update_authority(
        ctx: Context<UpdateAuthority>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let metadata = &mut ctx.accounts.metadata;
        let old_authority = metadata.update_authority;
        metadata.update_authority = new_authority;
        
        msg!("✅ Update authority transferred to: {}", new_authority);
        
        emit!(AuthorityTransferredEvent {
            mint: metadata.mint,
            old_authority,
            new_authority,
        });
        
        Ok(())
    }

    /// Burn metadata (makes NFT non-standard)
    pub fn burn_metadata(
        ctx: Context<BurnMetadata>,
    ) -> Result<()> {
        msg!("✅ Metadata burned for mint: {}", ctx.accounts.metadata.mint);
        
        emit!(MetadataBurnedEvent {
            mint: ctx.accounts.metadata.mint,
            metadata: ctx.accounts.metadata.key(),
        });
        
        Ok(())
    }
}

// ========== ACCOUNT CONTEXTS ==========

#[derive(Accounts)]
pub struct CreateMetadata<'info> {
    #[account(
        init,
        payer = payer,
        space = 8 + Metadata::SPACE,
        seeds = [b"metadata", mint.key().as_ref()],
        bump
    )]
    pub metadata: Account<'info, Metadata>,
    
    pub mint: Account<'info, Mint>,
    
    /// CHECK: This is the update authority
    pub update_authority: Signer<'info>,
    
    #[account(mut)]
    pub payer: Signer<'info>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct UpdateMetadata<'info> {
    #[account(
        mut,
        seeds = [b"metadata", metadata.mint.as_ref()],
        bump,
        has_one = update_authority,
    )]
    pub metadata: Account<'info, Metadata>,
    
    pub update_authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct UpdateAuthority<'info> {
    #[account(
        mut,
        seeds = [b"metadata", metadata.mint.as_ref()],
        bump,
        has_one = update_authority,
    )]
    pub metadata: Account<'info, Metadata>,
    
    pub update_authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct BurnMetadata<'info> {
    #[account(
        mut,
        close = update_authority,
        seeds = [b"metadata", metadata.mint.as_ref()],
        bump,
        has_one = update_authority,
    )]
    pub metadata: Account<'info, Metadata>,
    
    #[account(mut)]
    pub update_authority: Signer<'info>,
}

// ========== STATE ==========

#[account]
pub struct Metadata {
    pub mint: Pubkey,                // 32 bytes - The NFT mint address
    pub update_authority: Pubkey,    // 32 bytes - Who can update this
    pub name: String,                // 4 + 32 bytes - NFT name
    pub symbol: String,              // 4 + 10 bytes - NFT symbol
    pub uri: String,                 // 4 + 200 bytes - IPFS/Arweave URI
    pub is_mutable: bool,            // 1 byte - Can be updated?
}

impl Metadata {
    pub const SPACE: usize = 32 + 32 + 4 + 32 + 4 + 10 + 4 + 200 + 1; // 319 bytes
}

// ========== EVENTS ==========

#[event]
pub struct MetadataCreatedEvent {
    pub mint: Pubkey,
    pub metadata: Pubkey,
    pub name: String,
    pub symbol: String,
    pub uri: String,
}

#[event]
pub struct MetadataUpdatedEvent {
    pub mint: Pubkey,
    pub metadata: Pubkey,
    pub name: String,
    pub symbol: String,
    pub uri: String,
}

#[event]
pub struct AuthorityTransferredEvent {
    pub mint: Pubkey,
    pub old_authority: Pubkey,
    pub new_authority: Pubkey,
}

#[event]
pub struct MetadataBurnedEvent {
    pub mint: Pubkey,
    pub metadata: Pubkey,
}

// ========== ERRORS ==========

#[error_code]
pub enum ErrorCode {
    #[msg("Name too long (max 32 characters)")]
    NameTooLong,
    #[msg("Symbol too long (max 10 characters)")]
    SymbolTooLong,
    #[msg("URI too long (max 200 characters)")]
    UriTooLong,
}

