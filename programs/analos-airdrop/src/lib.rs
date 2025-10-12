use anchor_lang::prelude::*;

declare_id!("11111111111111111111111111111111"); // Placeholder ID

#[program]
pub mod analos_airdrop {
    use super::*;

    pub fn initialize(_ctx: Context<Initialize>) -> Result<()> {
        msg!("Analos Airdrop Program - Placeholder");
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
