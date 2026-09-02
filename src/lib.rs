//! BitGuard Lock V1 — Solana (Anchor).
//! Token + fungible LP (AMM/CPMM) vaults. Position NFT locks are compiled but DISABLED.
//! Immutable lock rules. No admin withdraw of customer assets. Fee is SOL, never the locked mint.

use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_interface::{
    self, Mint, TokenAccount, TokenInterface, TransferChecked,
};

declare_id!("FgTanvmeJJwfw8wbDFf5LMz2Hhrqgz1XAKAZsqDWgeKJ");

#[cfg(not(feature = "no-entrypoint"))]
use solana_security_txt::security_txt;

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "BitGuard Locker",
    project_url: "https://locker.bbspectrum.com",
    contacts: "email:security@bbspectrum.com,link:https://locker.bbspectrum.com",
    policy: "https://locker.bbspectrum.com/#docs",
    preferred_languages: "en",
    source_code: "https://locker.bbspectrum.com",
    auditors: "None"
}

pub const LOCK_SEED: &[u8] = b"bitguard-lock";
pub const CONFIG_SEED: &[u8] = b"bitguard-config";
/// 5 minutes — fee / treasury changes go live after this delay.
pub const FEE_TIMELOCK_SECS: i64 = 5 * 60;
pub const TITLE_MAX: usize = 80;

#[program]
pub mod bitguard_lock {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, fee_lamports: u64) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        cfg.authority = ctx.accounts.authority.key();
        cfg.fee_treasury = ctx.accounts.fee_treasury.key();
        cfg.pending_treasury = Pubkey::default();
        cfg.fee_lamports = fee_lamports;
        cfg.pending_fee_lamports = 0;
        cfg.fee_effective_ts = 0;
        cfg.treasury_effective_ts = 0;
        cfg.next_id = 1;
        cfg.bump = ctx.bumps.config;
        Ok(())
    }

    /// Queue a new SOL service fee. Anyone may `apply_fee` after 5 minutes.
    pub fn propose_fee(ctx: Context<Admin>, new_fee: u64) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        cfg.pending_fee_lamports = new_fee;
        cfg.fee_effective_ts = Clock::get()?.unix_timestamp + FEE_TIMELOCK_SECS;
        Ok(())
    }

    pub fn apply_fee(ctx: Context<ApplyConfig>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require!(cfg.fee_effective_ts != 0, BgError::NoPendingFee);
        require!(Clock::get()?.unix_timestamp >= cfg.fee_effective_ts, BgError::Timelock);
        cfg.fee_lamports = cfg.pending_fee_lamports;
        cfg.fee_effective_ts = 0;
        Ok(())
    }

    pub fn propose_treasury(ctx: Context<Admin>, new_treasury: Pubkey) -> Result<()> {
        require!(new_treasury != Pubkey::default(), BgError::Zero);
        let cfg = &mut ctx.accounts.config;
        cfg.pending_treasury = new_treasury;
        cfg.treasury_effective_ts = Clock::get()?.unix_timestamp + FEE_TIMELOCK_SECS;
        Ok(())
    }

    pub fn apply_treasury(ctx: Context<ApplyConfig>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require!(cfg.treasury_effective_ts != 0, BgError::NoPendingFee);
        require!(Clock::get()?.unix_timestamp >= cfg.treasury_effective_ts, BgError::Timelock);
        require!(cfg.pending_treasury != Pubkey::default(), BgError::Zero);
        cfg.fee_treasury = cfg.pending_treasury;
        cfg.pending_treasury = Pubkey::default();
        cfg.treasury_effective_ts = 0;
        Ok(())
    }

    /// Lock SPL / Token-2022 / Raydium AMM-CPMM LP mints into a PDA-owned vault.
    /// `is_lp`: false = token lock, true = fungible LP lock. Vesting via `vesting.enabled`.
    pub fn create_token_lock(
        ctx: Context<CreateTokenLock>,
        amount: u64,
        unlock_ts: i64,
        title: String,
        is_lp: bool,
        vesting: VestingArgs,
    ) -> Result<()> {
        require!(amount > 0, BgError::Amount);
        require!(title.len() > 0 && title.len() <= TITLE_MAX, BgError::Title);
        let now = Clock::get()?.unix_timestamp;
        if !vesting.enabled {
            require!(unlock_ts > now + 3600, BgError::UnlockSoon);
        } else {
            require!(vesting.tge_bps <= 10_000, BgError::Tge);
            require!(vesting.tge_ts >= now, BgError::TgePast);
        }

        let fee = ctx.accounts.config.fee_lamports;
        require!(ctx.accounts.owner.lamports() >= fee, BgError::Fee);

        require!(ctx.accounts.owner_ata.mint == ctx.accounts.mint.key(), BgError::MintMismatch);
        require!(ctx.accounts.owner_ata.owner == ctx.accounts.owner.key(), BgError::AtaOwner);
        require!(ctx.accounts.vault.mint == ctx.accounts.mint.key(), BgError::MintMismatch);

        let id = ctx.accounts.config.next_id;

        let before = ctx.accounts.vault.amount;
        token_interface::transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.owner_ata.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                    authority: ctx.accounts.owner.to_account_info(),
                },
            ),
            amount,
            ctx.accounts.mint.decimals,
        )?;
        ctx.accounts.vault.reload()?;
        let received = ctx.accounts.vault.amount.saturating_sub(before);
        require!(received > 0, BgError::ReceivedZero);

        let lock = &mut ctx.accounts.lock;
        lock.id = id;
        lock.owner = ctx.accounts.owner.key();
        lock.creator = ctx.accounts.owner.key();
        lock.pending_owner = Pubkey::default();
        lock.mint = ctx.accounts.mint.key();
        lock.vault = ctx.accounts.vault.key();
        lock.amount = received;
        lock.unlock_ts = if vesting.enabled {
            final_vest_time(&vesting)
        } else {
            unlock_ts
        };
        lock.created_ts = now;
        lock.title = title;
        lock.kind = if vesting.enabled {
            LockKind::Vesting
        } else if is_lp {
            LockKind::Lp
        } else {
            LockKind::Token
        };
        lock.position_kind = if is_lp { PositionKind::AmmLp } else { PositionKind::None };
        lock.dex = String::new();
        lock.withdrawn = false;
        lock.claimed = 0;
        lock.vesting = vesting;
        lock.bump = ctx.bumps.lock;

        ctx.accounts.config.next_id = id.checked_add(1).ok_or(BgError::Overflow)?;

        if fee > 0 {
            system_program::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    system_program::Transfer {
                        from: ctx.accounts.owner.to_account_info(),
                        to: ctx.accounts.fee_treasury.to_account_info(),
                    },
                ),
                fee,
            )?;
        }

        emit!(LockCreated {
            id,
            owner: lock.owner,
            mint: lock.mint,
            amount: received,
            unlock_ts: lock.unlock_ts,
            kind: lock.kind as u8,
        });
        Ok(())
    }

    pub fn create_position_lock(
        _ctx: Context<CreatePositionLock>,
        _unlock_ts: i64,
        _title: String,
        _dex: String,
    ) -> Result<()> {
        err!(BgError::PositionLocksDisabled)
    }

    pub fn extend_unlock(ctx: Context<OwnerLock>, new_unlock: i64) -> Result<()> {
        let lock = &mut ctx.accounts.lock;
        require!(!lock.withdrawn, BgError::Completed);
        require!(new_unlock > lock.unlock_ts, BgError::MustExtend);
        let old = lock.unlock_ts;
        lock.unlock_ts = new_unlock;
        emit!(LockExtended {
            id: lock.id,
            old,
            new_unlock
        });
        Ok(())
    }

    pub fn transfer_ownership(ctx: Context<OwnerLock>, new_owner: Pubkey) -> Result<()> {
        require!(new_owner != Pubkey::default(), BgError::Zero);
        require!(new_owner != ctx.accounts.owner.key(), BgError::Zero);
        ctx.accounts.lock.pending_owner = new_owner;
        Ok(())
    }

    pub fn accept_ownership(ctx: Context<AcceptOwnership>) -> Result<()> {
        let lock = &mut ctx.accounts.lock;
        require!(!lock.withdrawn, BgError::Completed);
        require!(lock.pending_owner == ctx.accounts.new_owner.key(), BgError::NotPending);
        lock.owner = ctx.accounts.new_owner.key();
        lock.pending_owner = Pubkey::default();
        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>) -> Result<()> {
        require!(!ctx.accounts.lock.withdrawn, BgError::Completed);
        require!(ctx.accounts.lock.kind != LockKind::Vesting, BgError::UseClaim);
        require!(
            Clock::get()?.unix_timestamp >= ctx.accounts.lock.unlock_ts,
            BgError::StillLocked
        );
        require!(ctx.accounts.owner_ata.mint == ctx.accounts.lock.mint, BgError::MintMismatch);
        require!(ctx.accounts.owner_ata.owner == ctx.accounts.owner.key(), BgError::AtaOwner);

        let amount = ctx.accounts.lock.amount;
        ctx.accounts.lock.withdrawn = true;

        let id_bytes = ctx.accounts.lock.id.to_le_bytes();
        let seeds: &[&[u8]] = &[
            LOCK_SEED,
            ctx.accounts.lock.creator.as_ref(),
            ctx.accounts.lock.mint.as_ref(),
            id_bytes.as_ref(),
            &[ctx.accounts.lock.bump],
        ];
        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.vault.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.owner_ata.to_account_info(),
                    authority: ctx.accounts.lock.to_account_info(),
                },
                &[seeds],
            ),
            amount,
            ctx.accounts.mint.decimals,
        )?;

        emit!(Withdrawn {
            id: ctx.accounts.lock.id,
            amount
        });
        Ok(())
    }

    pub fn claim_vested(ctx: Context<Withdraw>) -> Result<()> {
        require!(ctx.accounts.lock.kind == LockKind::Vesting, BgError::NotVesting);
        require!(!ctx.accounts.lock.withdrawn, BgError::Completed);
        require!(ctx.accounts.owner_ata.mint == ctx.accounts.lock.mint, BgError::MintMismatch);
        require!(ctx.accounts.owner_ata.owner == ctx.accounts.owner.key(), BgError::AtaOwner);

        let now = Clock::get()?.unix_timestamp;
        let releasable = vested_amount(&ctx.accounts.lock, now)?.saturating_sub(ctx.accounts.lock.claimed);
        require!(releasable > 0, BgError::NothingClaimable);

        ctx.accounts.lock.claimed = ctx.accounts.lock.claimed.saturating_add(releasable);
        if ctx.accounts.lock.claimed >= ctx.accounts.lock.amount {
            ctx.accounts.lock.withdrawn = true;
        }

        let id_bytes = ctx.accounts.lock.id.to_le_bytes();
        let seeds: &[&[u8]] = &[
            LOCK_SEED,
            ctx.accounts.lock.creator.as_ref(),
            ctx.accounts.lock.mint.as_ref(),
            id_bytes.as_ref(),
            &[ctx.accounts.lock.bump],
        ];
        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.vault.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.owner_ata.to_account_info(),
                    authority: ctx.accounts.lock.to_account_info(),
                },
                &[seeds],
            ),
            releasable,
            ctx.accounts.mint.decimals,
        )?;

        emit!(Withdrawn {
            id: ctx.accounts.lock.id,
            amount: releasable
        });
        Ok(())
    }
}

fn final_vest_time(v: &VestingArgs) -> i64 {
    if v.cycle_bps == 0 {
        return if v.cliff_ts > v.tge_ts { v.cliff_ts } else { v.tge_ts };
    }
    let remaining = 10_000u32.saturating_sub(v.tge_bps as u32);
    let cycles = (remaining + v.cycle_bps as u32 - 1) / (v.cycle_bps as u32).max(1);
    let start = if v.cliff_ts > v.tge_ts { v.cliff_ts } else { v.tge_ts };
    start.saturating_add((cycles as u64).saturating_mul(v.cycle_seconds) as i64)
}

fn vested_amount(lock: &LockAccount, now: i64) -> Result<u64> {
    let v = &lock.vesting;
    if !v.enabled {
        return Ok(if now >= lock.unlock_ts { lock.amount } else { 0 });
    }
    if now < v.tge_ts {
        return Ok(0);
    }
    let mut released = lock.amount.saturating_mul(v.tge_bps as u64) / 10_000;
    if v.cliff_ts != 0 && now < v.cliff_ts {
        return Ok(released);
    }
    if v.cycle_seconds == 0 || v.cycle_bps == 0 {
        if v.cliff_ts != 0 && now >= v.cliff_ts {
            return Ok(lock.amount);
        }
        return Ok(released);
    }
    let start = if v.cliff_ts > v.tge_ts { v.cliff_ts } else { v.tge_ts };
    if now <= start {
        return Ok(released);
    }
    let cycles = ((now - start) as u64) / v.cycle_seconds;
    released = released.saturating_add(
        lock.amount.saturating_mul(v.cycle_bps as u64).saturating_mul(cycles) / 10_000,
    );
    Ok(released.min(lock.amount))
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum LockKind {
    Token = 0,
    Lp = 1,
    Vesting = 2,
    Position = 3,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum PositionKind {
    None = 0,
    AmmLp = 1,
    Clmm = 2,
    Dlmm = 3,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct VestingArgs {
    pub enabled: bool,
    pub tge_bps: u16,
    pub tge_ts: i64,
    pub cliff_ts: i64,
    pub cycle_seconds: u64,
    pub cycle_bps: u16,
}

#[account]
pub struct GlobalConfig {
    pub authority: Pubkey,
    pub fee_treasury: Pubkey,
    pub pending_treasury: Pubkey,
    pub fee_lamports: u64,
    pub pending_fee_lamports: u64,
    pub fee_effective_ts: i64,
    pub treasury_effective_ts: i64,
    pub next_id: u64,
    pub bump: u8,
}

#[account]
pub struct LockAccount {
    pub id: u64,
    pub owner: Pubkey,
    pub creator: Pubkey,
    pub pending_owner: Pubkey,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub amount: u64,
    pub unlock_ts: i64,
    pub created_ts: i64,
    pub title: String,
    pub kind: LockKind,
    pub position_kind: PositionKind,
    pub dex: String,
    pub withdrawn: bool,
    pub claimed: u64,
    pub vesting: VestingArgs,
    pub bump: u8,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    /// CHECK: SOL fee recipient. Must be a system account that can receive lamports.
    #[account(mut)]
    pub fee_treasury: UncheckedAccount<'info>,
    #[account(
        init,
        payer = authority,
        space = 8 + 32 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 1,
        seeds = [CONFIG_SEED],
        bump
    )]
    pub config: Account<'info, GlobalConfig>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Admin<'info> {
    pub authority: Signer<'info>,
    #[account(mut, has_one = authority)]
    pub config: Account<'info, GlobalConfig>,
}

#[derive(Accounts)]
pub struct ApplyConfig<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GlobalConfig>,
}

#[derive(Accounts)]
pub struct CreateTokenLock<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GlobalConfig>,
    /// CHECK: must match config.fee_treasury; receives SOL, never the locked mint.
    #[account(mut, address = config.fee_treasury @ BgError::BadTreasury)]
    pub fee_treasury: UncheckedAccount<'info>,
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(mut)]
    pub owner_ata: InterfaceAccount<'info, TokenAccount>,
    #[account(
        init,
        payer = owner,
        associated_token::mint = mint,
        associated_token::authority = lock,
        associated_token::token_program = token_program
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,
    #[account(
        init,
        payer = owner,
        space = 8 + 512,
        seeds = [LOCK_SEED, owner.key().as_ref(), mint.key().as_ref(), &config.next_id.to_le_bytes()],
        bump
    )]
    pub lock: Account<'info, LockAccount>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CreatePositionLock<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, GlobalConfig>,
    /// CHECK: unused V1
    pub position_mint: UncheckedAccount<'info>,
    /// CHECK: unused V1
    pub position_vault: UncheckedAccount<'info>,
    #[account(
        init,
        payer = owner,
        space = 8 + 512,
        seeds = [LOCK_SEED, owner.key().as_ref(), position_mint.key().as_ref(), &config.next_id.to_le_bytes()],
        bump
    )]
    pub lock: Account<'info, LockAccount>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct OwnerLock<'info> {
    pub owner: Signer<'info>,
    #[account(mut, has_one = owner)]
    pub lock: Account<'info, LockAccount>,
}

#[derive(Accounts)]
pub struct AcceptOwnership<'info> {
    pub new_owner: Signer<'info>,
    #[account(mut)]
    pub lock: Account<'info, LockAccount>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    pub owner: Signer<'info>,
    #[account(
        mut,
        has_one = owner,
        has_one = mint,
        has_one = vault,
    )]
    pub lock: Account<'info, LockAccount>,
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(mut)]
    pub vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub owner_ata: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
}

#[event]
pub struct LockCreated {
    pub id: u64,
    pub owner: Pubkey,
    pub mint: Pubkey,
    pub amount: u64,
    pub unlock_ts: i64,
    pub kind: u8,
}

#[event]
pub struct LockExtended {
    pub id: u64,
    pub old: i64,
    pub new_unlock: i64,
}

#[event]
pub struct Withdrawn {
    pub id: u64,
    pub amount: u64,
}

#[error_code]
pub enum BgError {
    #[msg("Insufficient service fee")]
    Fee,
    #[msg("Invalid amount")]
    Amount,
    #[msg("Invalid title")]
    Title,
    #[msg("Unlock too soon")]
    UnlockSoon,
    #[msg("New date must be later")]
    MustExtend,
    #[msg("Lock already completed")]
    Completed,
    #[msg("Still locked")]
    StillLocked,
    #[msg("Use claim_vested")]
    UseClaim,
    #[msg("Not a vesting lock")]
    NotVesting,
    #[msg("Nothing claimable")]
    NothingClaimable,
    #[msg("Not pending owner")]
    NotPending,
    #[msg("Zero address")]
    Zero,
    #[msg("No pending change")]
    NoPendingFee,
    #[msg("Timelock not elapsed (5 minutes)")]
    Timelock,
    #[msg("Overflow")]
    Overflow,
    #[msg("Mint mismatch")]
    MintMismatch,
    #[msg("ATA owner mismatch")]
    AtaOwner,
    #[msg("Vault received 0")]
    ReceivedZero,
    #[msg("Fee treasury mismatch")]
    BadTreasury,
    #[msg("TGE bps invalid")]
    Tge,
    #[msg("TGE time in the past")]
    TgePast,
    #[msg("Position NFT locks are disabled in V1")]
    PositionLocksDisabled,
}