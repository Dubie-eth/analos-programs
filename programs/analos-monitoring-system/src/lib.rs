use anchor_lang::prelude::*;

// Security.txt implementation for program verification
#[cfg(not(feature = "no-entrypoint"))]
use {default_env::default_env, solana_security_txt::security_txt};

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Analos Monitoring System",
    project_url: "https://github.com/Dubie-eth/analos-programs",
    contacts: "email:security@analos.io,twitter:@EWildn,telegram:t.me/Dubie_420",
    policy: "https://github.com/Dubie-eth/analos-programs/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/Dubie-eth/analos-programs",
    source_revision: "7PT1ubRGFWXFCmZTpsa9gtm9GZf8BaYTkSd7gE8VcXdG",
    source_release: "v1.0.0",
    auditors: "None",
    acknowledgements: "Thank you to all security researchers who help keep Analos secure!"
}

declare_id!("7PT1ubRGFWXFCmZTpsa9gtm9GZf8BaYTkSd7gE8VcXdG");

/// Advanced Monitoring and Alerting System for NFT Launchpad
/// - Real-time security monitoring
/// - Automated alerting
/// - Risk assessment
/// - Audit trail management
#[program]
pub mod analos_monitoring {
    use super::*;

    /// Initialize monitoring system
    pub fn initialize_monitoring(
        ctx: Context<InitializeMonitoring>,
        alert_thresholds: AlertThresholds,
        monitoring_config: MonitoringConfig,
    ) -> Result<()> {
        let monitoring = &mut ctx.accounts.monitoring_system;
        monitoring.authority = ctx.accounts.authority.key();
        monitoring.alert_thresholds = alert_thresholds;
        monitoring.config = monitoring_config;
        monitoring.is_active = true;
        monitoring.created_at = Clock::get()?.unix_timestamp;
        monitoring.total_events = 0;
        monitoring.total_alerts = 0;
        
        emit!(MonitoringInitializedEvent {
            authority: monitoring.authority,
            created_at: monitoring.created_at,
            alert_thresholds: monitoring.alert_thresholds.clone(),
        });
        
        msg!("✅ Monitoring system initialized");
        Ok(())
    }

    /// Record security event
    pub fn record_event(
        ctx: Context<RecordEvent>,
        event_type: String,
        severity: String,
        user: Pubkey,
        details: String,
        risk_level: u8,
        program_id: Pubkey,
    ) -> Result<()> {
        let monitoring = &mut ctx.accounts.monitoring_system;
        let current_time = Clock::get()?.unix_timestamp;
        
        // Create event record
        let event = SecurityEventRecord {
            id: monitoring.total_events,
            event_type: event_type.clone(),
            severity: severity.clone(),
            user,
            timestamp: current_time,
            details: details.clone(),
            risk_level,
            program_id,
            processed: false,
        };
        
        // Store event
        monitoring.events.push(event);
        monitoring.total_events = monitoring.total_events.saturating_add(1);
        
        // Check if alert should be triggered
        let should_alert = check_alert_conditions(
            &monitoring.alert_thresholds,
            &event_type,
            &severity,
            risk_level,
            &monitoring.events,
        );
        
        if should_alert {
            // Create alert
            let alert = AlertRecord {
                id: monitoring.total_alerts,
                event_id: monitoring.total_events - 1,
                alert_type: get_alert_type(&event_type, risk_level),
                severity: severity.clone(),
                triggered_at: current_time,
                resolved_at: None,
                is_resolved: false,
                escalation_level: get_escalation_level(risk_level),
            };
            
            monitoring.alerts.push(alert);
            monitoring.total_alerts = monitoring.total_alerts.saturating_add(1);
            
            emit!(AlertTriggeredEvent {
                alert_id: monitoring.total_alerts - 1,
                event_type: event_type.clone(),
                severity: severity.clone(),
                user,
                risk_level,
                triggered_at: current_time,
            });
            
            msg!("🚨 ALERT TRIGGERED: {} - {}", event_type, severity);
        }
        
        emit!(EventRecordedEvent {
            event_id: monitoring.total_events - 1,
            event_type,
            severity,
            user,
            risk_level,
            recorded_at: current_time,
        });
        
        Ok(())
    }

    /// Resolve alert
    pub fn resolve_alert(
        ctx: Context<ResolveAlert>,
        alert_id: u64,
        resolution_notes: String,
    ) -> Result<()> {
        let monitoring = &mut ctx.accounts.monitoring_system;
        
        require!(alert_id < monitoring.total_alerts, ErrorCode::InvalidAlertId);
        require!(!monitoring.alerts[alert_id as usize].is_resolved, ErrorCode::AlertAlreadyResolved);
        
        let alert = &mut monitoring.alerts[alert_id as usize];
        alert.is_resolved = true;
        alert.resolved_at = Some(Clock::get()?.unix_timestamp);
        
        emit!(AlertResolvedEvent {
            alert_id,
            resolved_by: ctx.accounts.authority.key(),
            resolved_at: alert.resolved_at.unwrap(),
            resolution_notes,
        });
        
        msg!("✅ Alert {} resolved", alert_id);
        Ok(())
    }

    /// Update alert thresholds
    pub fn update_thresholds(
        ctx: Context<UpdateThresholds>,
        new_thresholds: AlertThresholds,
    ) -> Result<()> {
        let monitoring = &mut ctx.accounts.monitoring_system;
        let old_thresholds = monitoring.alert_thresholds.clone();
        monitoring.alert_thresholds = new_thresholds.clone();
        
        emit!(ThresholdsUpdatedEvent {
            authority: ctx.accounts.authority.key(),
            old_thresholds,
            new_thresholds,
            updated_at: Clock::get()?.unix_timestamp,
        });
        
        msg!("✅ Alert thresholds updated");
        Ok(())
    }

    /// Get security statistics
    pub fn get_security_stats(ctx: Context<GetSecurityStats>) -> Result<()> {
        let monitoring = &ctx.accounts.monitoring_system;
        let current_time = Clock::get()?.unix_timestamp;
        
        // Calculate statistics
        let active_alerts = monitoring.alerts.iter().filter(|a| !a.is_resolved).count();
        let high_risk_events = monitoring.events.iter().filter(|e| e.risk_level >= 4).count();
        let events_last_hour = monitoring.events.iter()
            .filter(|e| current_time - e.timestamp <= 3600)
            .count();
        
        emit!(SecurityStatsEvent {
            total_events: monitoring.total_events,
            total_alerts: monitoring.total_alerts,
            active_alerts: active_alerts as u64,
            high_risk_events: high_risk_events as u64,
            events_last_hour: events_last_hour as u64,
            generated_at: current_time,
        });
        
        msg!("📊 Security stats: {} events, {} alerts, {} active", 
             monitoring.total_events, monitoring.total_alerts, active_alerts);
        Ok(())
    }

    /// Emergency shutdown
    pub fn emergency_shutdown(ctx: Context<EmergencyShutdown>) -> Result<()> {
        let monitoring = &mut ctx.accounts.monitoring_system;
        monitoring.is_active = false;
        monitoring.shutdown_at = Some(Clock::get()?.unix_timestamp);
        monitoring.shutdown_by = ctx.accounts.authority.key();
        
        emit!(EmergencyShutdownEvent {
            shutdown_by: ctx.accounts.authority.key(),
            shutdown_at: monitoring.shutdown_at.unwrap(),
            reason: "Emergency shutdown activated".to_string(),
        });
        
        msg!("🚨 EMERGENCY SHUTDOWN ACTIVATED");
        Ok(())
    }
}

// ========== HELPER FUNCTIONS ==========

fn check_alert_conditions(
    thresholds: &AlertThresholds,
    event_type: &str,
    severity: &str,
    risk_level: u8,
    events: &Vec<SecurityEventRecord>,
) -> bool {
    let current_time = Clock::get().unwrap().unix_timestamp;
    
    // Check risk level threshold
    if risk_level >= thresholds.risk_level_threshold {
        return true;
    }
    
    // Check severity threshold
    if severity == "CRITICAL" && thresholds.alert_on_critical {
        return true;
    }
    
    // Check event frequency
    let recent_events = events.iter()
        .filter(|e| current_time - e.timestamp <= thresholds.time_window)
        .count();
    
    if recent_events >= thresholds.event_frequency_threshold {
        return true;
    }
    
    // Check specific event types
    match event_type {
        "EMERGENCY_PAUSE" | "EMERGENCY_UNLOCK" | "EMERGENCY_WITHDRAWAL" => true,
        "RATE_LIMIT_EXCEEDED" if thresholds.alert_on_rate_limit => true,
        "UNAUTHORIZED_ACCESS" => true,
        _ => false,
    }
}

fn get_alert_type(event_type: &str, risk_level: u8) -> String {
    match (event_type, risk_level) {
        (_, 5) => "CRITICAL_SECURITY".to_string(),
        (_, 4) => "HIGH_RISK".to_string(),
        (_, 3) => "MEDIUM_RISK".to_string(),
        ("RATE_LIMIT_EXCEEDED", _) => "RATE_LIMIT".to_string(),
        ("UNAUTHORIZED_ACCESS", _) => "UNAUTHORIZED_ACCESS".to_string(),
        _ => "GENERAL".to_string(),
    }
}

fn get_escalation_level(risk_level: u8) -> u8 {
    match risk_level {
        5 => 3, // Critical - immediate escalation
        4 => 2, // High - escalate within 1 hour
        3 => 1, // Medium - escalate within 4 hours
        _ => 0, // Low - no escalation
    }
}

// ========== ACCOUNTS ==========

#[derive(Accounts)]
pub struct InitializeMonitoring<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + MonitoringSystem::SPACE,
        seeds = [b"monitoring"],
        bump
    )]
    pub monitoring_system: Account<'info, MonitoringSystem>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RecordEvent<'info> {
    #[account(
        mut,
        seeds = [b"monitoring"],
        bump
    )]
    pub monitoring_system: Account<'info, MonitoringSystem>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct ResolveAlert<'info> {
    #[account(
        mut,
        seeds = [b"monitoring"],
        bump
    )]
    pub monitoring_system: Account<'info, MonitoringSystem>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct UpdateThresholds<'info> {
    #[account(
        mut,
        seeds = [b"monitoring"],
        bump
    )]
    pub monitoring_system: Account<'info, MonitoringSystem>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct GetSecurityStats<'info> {
    #[account(
        seeds = [b"monitoring"],
        bump
    )]
    pub monitoring_system: Account<'info, MonitoringSystem>,
}

#[derive(Accounts)]
pub struct EmergencyShutdown<'info> {
    #[account(
        mut,
        seeds = [b"monitoring"],
        bump
    )]
    pub monitoring_system: Account<'info, MonitoringSystem>,
    
    pub authority: Signer<'info>,
}

// ========== STATE ==========

#[account]
pub struct MonitoringSystem {
    pub authority: Pubkey,                    // 32
    pub alert_thresholds: AlertThresholds,    // 40
    pub config: MonitoringConfig,             // 24
    pub is_active: bool,                      // 1
    pub created_at: i64,                      // 8
    pub total_events: u64,                    // 8
    pub total_alerts: u64,                    // 8
    pub shutdown_at: Option<i64>,            // 9
    pub shutdown_by: Pubkey,                  // 32
    pub events: Vec<SecurityEventRecord>,     // 4 + (N * 128)
    pub alerts: Vec<AlertRecord>,             // 4 + (N * 64)
}

impl MonitoringSystem {
    pub const SPACE: usize = 32 + 40 + 24 + 1 + 8 + 8 + 8 + 9 + 32 + 4 + (1000 * 128) + 4 + (100 * 64); // ~130KB max
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct AlertThresholds {
    pub risk_level_threshold: u8,             // 1
    pub event_frequency_threshold: u64,       // 8
    pub time_window: i64,                     // 8
    pub alert_on_critical: bool,              // 1
    pub alert_on_rate_limit: bool,            // 1
    pub alert_on_unauthorized: bool,          // 1
    pub escalation_timeout: i64,              // 8
    pub max_events_per_hour: u64,             // 8
    pub max_alerts_per_hour: u64,             // 8
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct MonitoringConfig {
    pub enable_real_time_monitoring: bool,    // 1
    pub enable_automated_alerts: bool,        // 1
    pub enable_audit_trail: bool,             // 1
    pub retention_days: u32,                  // 4
    pub max_events: u32,                      // 4
    pub max_alerts: u32,                      // 4
    pub alert_cooldown: i64,                  // 8
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct SecurityEventRecord {
    pub id: u64,                              // 8
    pub event_type: String,                   // 4 + 32
    pub severity: String,                     // 4 + 16
    pub user: Pubkey,                         // 32
    pub timestamp: i64,                       // 8
    pub details: String,                      // 4 + 64
    pub risk_level: u8,                       // 1
    pub program_id: Pubkey,                   // 32
    pub processed: bool,                      // 1
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct AlertRecord {
    pub id: u64,                              // 8
    pub event_id: u64,                        // 8
    pub alert_type: String,                   // 4 + 32
    pub severity: String,                     // 4 + 16
    pub triggered_at: i64,                    // 8
    pub resolved_at: Option<i64>,            // 9
    pub is_resolved: bool,                    // 1
    pub escalation_level: u8,                 // 1
}

// ========== EVENTS ==========

#[event]
pub struct MonitoringInitializedEvent {
    pub authority: Pubkey,
    pub created_at: i64,
    pub alert_thresholds: AlertThresholds,
}

#[event]
pub struct EventRecordedEvent {
    pub event_id: u64,
    pub event_type: String,
    pub severity: String,
    pub user: Pubkey,
    pub risk_level: u8,
    pub recorded_at: i64,
}

#[event]
pub struct AlertTriggeredEvent {
    pub alert_id: u64,
    pub event_type: String,
    pub severity: String,
    pub user: Pubkey,
    pub risk_level: u8,
    pub triggered_at: i64,
}

#[event]
pub struct AlertResolvedEvent {
    pub alert_id: u64,
    pub resolved_by: Pubkey,
    pub resolved_at: i64,
    pub resolution_notes: String,
}

#[event]
pub struct ThresholdsUpdatedEvent {
    pub authority: Pubkey,
    pub old_thresholds: AlertThresholds,
    pub new_thresholds: AlertThresholds,
    pub updated_at: i64,
}

#[event]
pub struct SecurityStatsEvent {
    pub total_events: u64,
    pub total_alerts: u64,
    pub active_alerts: u64,
    pub high_risk_events: u64,
    pub events_last_hour: u64,
    pub generated_at: i64,
}

#[event]
pub struct EmergencyShutdownEvent {
    pub shutdown_by: Pubkey,
    pub shutdown_at: i64,
    pub reason: String,
}

// ========== ERRORS ==========

#[error_code]
pub enum ErrorCode {
    #[msg("Invalid alert ID")]
    InvalidAlertId,
    #[msg("Alert already resolved")]
    AlertAlreadyResolved,
    #[msg("Monitoring system not active")]
    SystemNotActive,
    #[msg("Unauthorized access")]
    Unauthorized,
    #[msg("Invalid configuration")]
    InvalidConfig,
}
