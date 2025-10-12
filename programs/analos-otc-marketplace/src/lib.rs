use anchor_lang::prelude::*;

declare_id!("11111111111111111111111111111111"); // Placeholder ID

#[program]
pub mod analos_otc_marketplace {
    use super::*;

    pub fn initialize(_ctx: Context<Initialize>) -> Result<()> {
        msg!("Analos OTC Marketplace Program - Placeholder");
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
