use anchor_lang::prelude::*;
use anchor_lang::solana_program::{keccak, pubkey};

// Security.txt implementation for program verification
#[cfg(not(feature = "no-entrypoint"))]
use {default_env::default_env, solana_security_txt::security_txt};

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Analos Rarity Oracle",
    project_url: "https://github.com/Dubie-eth/analos-programs",
    contacts: "email:support@launchonlos.fun,twitter:@EWildn,telegram:t.me/Dubie_420",
    policy: "https://github.com/Dubie-eth/analos-programs/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/Dubie-eth/analos-programs",
    source_revision: "3cnHMbD3Y88BZbxEPzv7WZGkN12X2bwKEP4FFtaVd5B2",
    source_release: "v1.0.0",
    auditors: "None",
    acknowledgements: "Thank you to all security researchers who help keep Analos secure!"
}

declare_id!("3cnHMbD3Y88BZbxEPzv7WZGkN12X2bwKEP4FFtaVd5B2");

/// Maximum rarity tiers
pub const MAX_RARITY_TIERS: usize = 10;
pub const MAX_METADATA_ATTRIBUTES: usize = 50;

#[program]
pub mod analos_rarity_oracle {
    use super::*;

    /// Initialize rarity configuration for a collection
    pub fn initialize_rarity_config(
        ctx: Context<InitializeRarityConfig>,
    ) -> Result<()> {
        let config = &mut ctx.accounts.rarity_config;
        
        config.collection_config = ctx.accounts.collection_config.key();
        config.authority = ctx.accounts.authority.key();
        config.oracle_authority = ctx.accounts.authority.key();
        config.total_revealed = 0;
        config.is_active = true;
        config.use_metadata_based = false;
        config.use_randomness = true;
        config.created_at = Clock::get()?.unix_timestamp;
        
        emit!(RarityConfigInitializedEvent {
            collection_config: config.collection_config,
            authority: config.authority,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Rarity config initialized for collection {}", config.collection_config);
        
        Ok(())
    }

    /// Add a rarity tier (Common, Rare, Mythic, etc.)
    pub fn add_rarity_tier(
        ctx: Context<AddRarityTier>,
        tier_id: u8,
        tier_name: String,
        token_multiplier: u64,
        probability_bps: u16,
        metadata_attributes: Vec<String>,
    ) -> Result<()> {
        let config = &mut ctx.accounts.rarity_config;
        let tier = &mut ctx.accounts.rarity_tier;
        
        require!(tier_id < MAX_RARITY_TIERS as u8, ErrorCode::InvalidTierId);
        require!(token_multiplier > 0 && token_multiplier <= 1000, ErrorCode::InvalidMultiplier);
        require!(probability_bps > 0 && probability_bps <= 10000, ErrorCode::InvalidProbability);
        require!(metadata_attributes.len() <= MAX_METADATA_ATTRIBUTES, ErrorCode::TooManyAttributes);
        
        tier.collection_config = config.collection_config;
        tier.tier_id = tier_id;
        tier.tier_name = tier_name.clone();
        tier.token_multiplier = token_multiplier;
        tier.probability_bps = probability_bps;
        tier.probability_cumulative_bps = 0; // Will be calculated later
        tier.total_count = 0;
        tier.max_count = 0; // 0 = unlimited
        tier.is_active = true;
        tier.created_at = Clock::get()?.unix_timestamp;
        
        emit!(RarityTierAddedEvent {
            collection_config: config.collection_config,
            tier_id,
            tier_name,
            token_multiplier,
            probability_bps,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Rarity tier {} '{}' added: {}x multiplier, {}% probability", 
            tier_id, tier.tier_name, token_multiplier, probability_bps / 100);
        
        Ok(())
    }

    /// Determine rarity for an NFT (called during reveal)
    pub fn determine_rarity(
        ctx: Context<DetermineRarity>,
        nft_mint: Pubkey,
        mint_index: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.rarity_config;
        let determination = &mut ctx.accounts.rarity_determination;
        
        require!(config.is_active, ErrorCode::OracleInactive);
        
        // Generate verifiable randomness
        let clock = Clock::get()?;
        let seed_data = [
            nft_mint.as_ref(),
            &mint_index.to_le_bytes(),
            &clock.unix_timestamp.to_le_bytes(),
            &clock.slot.to_le_bytes(),
            config.collection_config.as_ref(),
        ].concat();
        let random_hash = keccak::hash(&seed_data);
        let random_bytes = random_hash.to_bytes();
        
        // Convert first 8 bytes to u64 for randomness
        let mut random_u64_bytes = [0u8; 8];
        random_u64_bytes.copy_from_slice(&random_bytes[0..8]);
        let random_value = u64::from_le_bytes(random_u64_bytes);
        
        // Map to 0-10000 for probability
        let random_bps = (random_value % 10000) as u16;
        
        // Determine tier based on probability (simplified - would query RarityTier accounts in production)
        let (rarity_tier, token_multiplier) = determine_tier_from_probability(random_bps)?;
        
        determination.nft_mint = nft_mint;
        determination.collection_config = config.collection_config;
        determination.rarity_tier = rarity_tier;
        determination.token_multiplier = token_multiplier;
        determination.random_seed = random_hash.to_bytes();
        determination.probability_roll = random_bps;
        determination.determined_at = Clock::get()?.unix_timestamp;
        determination.determined_by = ctx.accounts.oracle_authority.key();
        
        config.total_revealed += 1;
        
        emit!(RarityDeterminedEvent {
            nft_mint,
            rarity_tier,
            token_multiplier,
            probability_roll: random_bps,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("NFT {} rarity determined: Tier {} with {}x multiplier", 
            nft_mint, rarity_tier, token_multiplier);
        
        Ok(())
    }

    /// Update rarity tier configuration
    pub fn update_rarity_tier(
        ctx: Context<UpdateRarityTier>,
        tier_id: u8,
        new_probability_bps: Option<u16>,
        new_multiplier: Option<u64>,
        new_max_count: Option<u64>,
    ) -> Result<()> {
        let tier = &mut ctx.accounts.rarity_tier;
        
        require!(tier.tier_id == tier_id, ErrorCode::InvalidTierId);
        
        if let Some(prob) = new_probability_bps {
            require!(prob > 0 && prob <= 10000, ErrorCode::InvalidProbability);
            tier.probability_bps = prob;
        }
        
        if let Some(mult) = new_multiplier {
            require!(mult > 0 && mult <= 1000, ErrorCode::InvalidMultiplier);
            tier.token_multiplier = mult;
        }
        
        if let Some(max) = new_max_count {
            tier.max_count = max;
        }
        
        emit!(RarityTierUpdatedEvent {
            collection_config: tier.collection_config,
            tier_id,
            new_probability_bps,
            new_multiplier,
            new_max_count,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Rarity tier {} updated", tier_id);
        
        Ok(())
    }

    /// Set metadata-based rarity (for specific attributes)
    pub fn set_metadata_rarity_mapping(
        ctx: Context<SetMetadataMapping>,
        tier_id: u8,
        attribute_name: String,
        attribute_values: Vec<String>,
    ) -> Result<()> {
        let mapping = &mut ctx.accounts.metadata_mapping;
        let config = &ctx.accounts.rarity_config;
        
        require!(tier_id < MAX_RARITY_TIERS as u8, ErrorCode::InvalidTierId);
        require!(attribute_values.len() > 0, ErrorCode::EmptyAttributes);
        
        mapping.collection_config = config.collection_config;
        mapping.tier_id = tier_id;
        mapping.attribute_name = attribute_name.clone();
        mapping.created_at = Clock::get()?.unix_timestamp;
        
        emit!(MetadataRarityMappingSetEvent {
            collection_config: config.collection_config,
            tier_id,
            attribute_name,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Metadata rarity mapping set for tier {}: {}", tier_id, attribute_name);
        
        Ok(())
    }

    /// Verify and update rarity distribution stats
    pub fn update_rarity_stats(
        ctx: Context<UpdateRarityStats>,
        tier_id: u8,
    ) -> Result<()> {
        let tier = &mut ctx.accounts.rarity_tier;
        
        require!(tier.tier_id == tier_id, ErrorCode::InvalidTierId);
        
        tier.total_count += 1;
        
        // Check if max count reached
        if tier.max_count > 0 && tier.total_count >= tier.max_count {
            tier.is_active = false;
            msg!("⚠️  Tier {} has reached max count and is now inactive", tier_id);
        }
        
        emit!(RarityStatsUpdatedEvent {
            collection_config: tier.collection_config,
            tier_id,
            total_count: tier.total_count,
            is_active: tier.is_active,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        Ok(())
    }

    /// Emergency: Override rarity (oracle authority only)
    pub fn override_rarity(
        ctx: Context<OverrideRarity>,
        nft_mint: Pubkey,
        new_rarity_tier: u8,
        new_multiplier: u64,
        reason: String,
    ) -> Result<()> {
        let determination = &mut ctx.accounts.rarity_determination;
        let config = &ctx.accounts.rarity_config;
        
        require!(determination.nft_mint == nft_mint, ErrorCode::InvalidNFTMint);
        require!(new_rarity_tier < MAX_RARITY_TIERS as u8, ErrorCode::InvalidTierId);
        require!(new_multiplier > 0 && new_multiplier <= 1000, ErrorCode::InvalidMultiplier);
        
        let old_tier = determination.rarity_tier;
        let old_multiplier = determination.token_multiplier;
        
        determination.rarity_tier = new_rarity_tier;
        determination.token_multiplier = new_multiplier;
        determination.determined_by = ctx.accounts.oracle_authority.key();
        determination.determined_at = Clock::get()?.unix_timestamp;
        
        emit!(RarityOverriddenEvent {
            nft_mint,
            old_tier,
            new_tier: new_rarity_tier,
            old_multiplier,
            new_multiplier,
            reason,
            overridden_by: ctx.accounts.oracle_authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("⚠️  Rarity overridden for {}: Tier {} → {}, Multiplier {}x → {}x", 
            nft_mint, old_tier, new_rarity_tier, old_multiplier, new_multiplier);
        
        Ok(())
    }
}

// ========== HELPER FUNCTIONS ==========

/// Determine tier from probability roll (simplified)
/// In production, this would query all RarityTier accounts and use cumulative probabilities
fn determine_tier_from_probability(random_bps: u16) -> Result<(u8, u64)> {
    // Example distribution (creator would configure these):
    // Common: 0-6999 (70%) = 1x
    // Uncommon: 7000-8499 (15%) = 5x
    // Rare: 8500-9499 (10%) = 10x
    // Epic: 9500-9799 (3%) = 50x
    // Legendary: 9800-9949 (1.5%) = 100x
    // Mythic: 9950-9999 (0.5%) = 1000x
    
    let (tier, multiplier) = if random_bps < 7000 {
        (0, 1)      // Common: 70%
    } else if random_bps < 8500 {
        (1, 5)      // Uncommon: 15%
    } else if random_bps < 9500 {
        (2, 10)     // Rare: 10%
    } else if random_bps < 9800 {
        (3, 50)     // Epic: 3%
    } else if random_bps < 9950 {
        (4, 100)    // Legendary: 1.5%
    } else {
        (5, 1000)   // Mythic: 0.5%
    };
    
    Ok((tier, multiplier))
}

// ========== ACCOUNT CONTEXTS ==========

#[derive(Accounts)]
pub struct InitializeRarityConfig<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + RarityConfig::INIT_SPACE,
        seeds = [b"rarity_config", collection_config.key().as_ref()],
        bump
    )]
    pub rarity_config: Account<'info, RarityConfig>,

    /// CHECK: NFT collection config
    pub collection_config: UncheckedAccount<'info>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AddRarityTier<'info> {
    #[account(
        mut,
        seeds = [b"rarity_config", rarity_config.collection_config.as_ref()],
        bump,
        has_one = authority,
    )]
    pub rarity_config: Account<'info, RarityConfig>,

    #[account(
        init,
        payer = authority,
        space = 8 + RarityTier::INIT_SPACE,
        seeds = [b"rarity_tier", rarity_config.key().as_ref(), &[tier_id]],
        bump
    )]
    pub rarity_tier: Account<'info, RarityTier>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(tier_id: u8)]
pub struct AddRarityTier<'info> {
    #[account(
        mut,
        seeds = [b"rarity_config", rarity_config.collection_config.as_ref()],
        bump,
        has_one = authority,
    )]
    pub rarity_config: Account<'info, RarityConfig>,

    #[account(
        init,
        payer = authority,
        space = 8 + RarityTier::INIT_SPACE,
        seeds = [b"rarity_tier", rarity_config.key().as_ref(), &[tier_id]],
        bump
    )]
    pub rarity_tier: Account<'info, RarityTier>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DetermineRarity<'info> {
    #[account(
        mut,
        seeds = [b"rarity_config", rarity_config.collection_config.as_ref()],
        bump,
    )]
    pub rarity_config: Account<'info, RarityConfig>,

    #[account(
        init,
        payer = oracle_authority,
        space = 8 + RarityDetermination::INIT_SPACE,
        seeds = [b"rarity_determination", rarity_config.key().as_ref(), nft_mint.key().as_ref()],
        bump
    )]
    pub rarity_determination: Account<'info, RarityDetermination>,

    /// CHECK: NFT mint account
    pub nft_mint: UncheckedAccount<'info>,

    #[account(mut)]
    pub oracle_authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateRarityTier<'info> {
    #[account(
        mut,
        seeds = [b"rarity_config", rarity_config.collection_config.as_ref()],
        bump,
        has_one = authority,
    )]
    pub rarity_config: Account<'info, RarityConfig>,

    #[account(
        mut,
        seeds = [b"rarity_tier", rarity_config.key().as_ref(), &[tier_id]],
        bump,
    )]
    pub rarity_tier: Account<'info, RarityTier>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(tier_id: u8)]
pub struct UpdateRarityTier<'info> {
    #[account(
        mut,
        seeds = [b"rarity_config", rarity_config.collection_config.as_ref()],
        bump,
        has_one = authority,
    )]
    pub rarity_config: Account<'info, RarityConfig>,

    #[account(
        mut,
        seeds = [b"rarity_tier", rarity_config.key().as_ref(), &[tier_id]],
        bump,
    )]
    pub rarity_tier: Account<'info, RarityTier>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetMetadataMapping<'info> {
    #[account(
        mut,
        seeds = [b"rarity_config", rarity_config.collection_config.as_ref()],
        bump,
        has_one = authority,
    )]
    pub rarity_config: Account<'info, RarityConfig>,

    #[account(
        init,
        payer = authority,
        space = 8 + MetadataRarityMapping::INIT_SPACE,
        seeds = [b"metadata_mapping", rarity_config.key().as_ref(), &[tier_id]],
        bump
    )]
    pub metadata_mapping: Account<'info, MetadataRarityMapping>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(tier_id: u8)]
pub struct SetMetadataMapping<'info> {
    #[account(
        mut,
        seeds = [b"rarity_config", rarity_config.collection_config.as_ref()],
        bump,
        has_one = authority,
    )]
    pub rarity_config: Account<'info, RarityConfig>,

    #[account(
        init,
        payer = authority,
        space = 8 + MetadataRarityMapping::INIT_SPACE,
        seeds = [b"metadata_mapping", rarity_config.key().as_ref(), &[tier_id]],
        bump
    )]
    pub metadata_mapping: Account<'info, MetadataRarityMapping>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateRarityStats<'info> {
    #[account(
        mut,
        seeds = [b"rarity_tier", rarity_config.key().as_ref(), &[tier_id]],
        bump,
    )]
    pub rarity_tier: Account<'info, RarityTier>,

    #[account(
        seeds = [b"rarity_config", rarity_config.collection_config.as_ref()],
        bump,
    )]
    pub rarity_config: Account<'info, RarityConfig>,
}

#[derive(Accounts)]
#[instruction(tier_id: u8)]
pub struct UpdateRarityStats<'info> {
    #[account(
        mut,
        seeds = [b"rarity_tier", rarity_config.key().as_ref(), &[tier_id]],
        bump,
    )]
    pub rarity_tier: Account<'info, RarityTier>,

    #[account(
        seeds = [b"rarity_config", rarity_config.collection_config.as_ref()],
        bump,
    )]
    pub rarity_config: Account<'info, RarityConfig>,
}

#[derive(Accounts)]
pub struct OverrideRarity<'info> {
    #[account(
        mut,
        seeds = [b"rarity_config", rarity_config.collection_config.as_ref()],
        bump,
        has_one = oracle_authority,
    )]
    pub rarity_config: Account<'info, RarityConfig>,

    #[account(
        mut,
        seeds = [b"rarity_determination", rarity_config.key().as_ref(), rarity_determination.nft_mint.as_ref()],
        bump,
    )]
    pub rarity_determination: Account<'info, RarityDetermination>,

    pub oracle_authority: Signer<'info>,
}

// ========== STATE ==========

#[account]
#[derive(InitSpace)]
pub struct RarityConfig {
    pub collection_config: Pubkey,
    pub authority: Pubkey,
    pub oracle_authority: Pubkey,
    pub total_revealed: u64,
    pub is_active: bool,
    pub use_metadata_based: bool,
    pub use_randomness: bool,
    pub created_at: i64,
}

#[account]
#[derive(InitSpace)]
pub struct RarityTier {
    pub collection_config: Pubkey,
    pub tier_id: u8,
    #[max_len(50)]
    pub tier_name: String,
    pub token_multiplier: u64,
    pub probability_bps: u16,
    pub probability_cumulative_bps: u16,
    pub total_count: u64,
    pub max_count: u64,
    pub is_active: bool,
    pub created_at: i64,
}

#[account]
#[derive(InitSpace)]
pub struct RarityDetermination {
    pub nft_mint: Pubkey,
    pub collection_config: Pubkey,
    pub rarity_tier: u8,
    pub token_multiplier: u64,
    pub random_seed: [u8; 32],
    pub probability_roll: u16,
    pub determined_at: i64,
    pub determined_by: Pubkey,
}

#[account]
#[derive(InitSpace)]
pub struct MetadataRarityMapping {
    pub collection_config: Pubkey,
    pub tier_id: u8,
    #[max_len(50)]
    pub attribute_name: String,
    pub created_at: i64,
}

// ========== EVENTS ==========

#[event]
pub struct RarityConfigInitializedEvent {
    pub collection_config: Pubkey,
    pub authority: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct RarityTierAddedEvent {
    pub collection_config: Pubkey,
    pub tier_id: u8,
    pub tier_name: String,
    pub token_multiplier: u64,
    pub probability_bps: u16,
    pub timestamp: i64,
}

#[event]
pub struct RarityDeterminedEvent {
    pub nft_mint: Pubkey,
    pub rarity_tier: u8,
    pub token_multiplier: u64,
    pub probability_roll: u16,
    pub timestamp: i64,
}

#[event]
pub struct RarityTierUpdatedEvent {
    pub collection_config: Pubkey,
    pub tier_id: u8,
    pub new_probability_bps: Option<u16>,
    pub new_multiplier: Option<u64>,
    pub new_max_count: Option<u64>,
    pub timestamp: i64,
}

#[event]
pub struct MetadataRarityMappingSetEvent {
    pub collection_config: Pubkey,
    pub tier_id: u8,
    pub attribute_name: String,
    pub timestamp: i64,
}

#[event]
pub struct RarityStatsUpdatedEvent {
    pub collection_config: Pubkey,
    pub tier_id: u8,
    pub total_count: u64,
    pub is_active: bool,
    pub timestamp: i64,
}

#[event]
pub struct RarityOverriddenEvent {
    pub nft_mint: Pubkey,
    pub old_tier: u8,
    pub new_tier: u8,
    pub old_multiplier: u64,
    pub new_multiplier: u64,
    pub reason: String,
    pub overridden_by: Pubkey,
    pub timestamp: i64,
}

// ========== ERRORS ==========

#[error_code]
pub enum ErrorCode {
    #[msg("Invalid tier ID (must be 0-9)")]
    InvalidTierId,
    #[msg("Invalid token multiplier (must be 1-1000)")]
    InvalidMultiplier,
    #[msg("Invalid probability (must be 1-10000)")]
    InvalidProbability,
    #[msg("Too many attributes (max 50)")]
    TooManyAttributes,
    #[msg("Oracle is inactive")]
    OracleInactive,
    #[msg("Invalid NFT mint")]
    InvalidNFTMint,
    #[msg("Empty attributes list")]
    EmptyAttributes,
}

