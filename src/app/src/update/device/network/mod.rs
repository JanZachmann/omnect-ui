pub mod config;
pub mod form;
pub mod verification;

pub use config::{handle_set_network_config, handle_set_network_config_response};
pub use form::{handle_network_form_start_edit, handle_network_form_update};
pub use verification::{
    handle_ack_rollback, handle_new_ip_check_tick, handle_new_ip_check_timeout,
};

/*
User applies config → ApplyingConfig
                          ↓
                     (backend responds)
                          ↓
        ┌─────────────────┴──────────────────┐
        │                                    │
  is_server_addr=false            is_server_addr=true
        │                                    │
        ↓                                    ↓
      Idle                          WaitingForNewIp
                                            ↓
                    ┌───────────────────────┼───────────────────────┐
                    │                       │                       │
              switching_to_dhcp       (polling new IP)        (new IP responds)
                    │                       │                       │
                    ↓                       ↓                       ↓
            (wait/manual nav)          (timeout)              NewIpReachable
                                            │                       │
                        ┌───────────────────┴──────┐               ↓
                        │                          │          (redirect)
                rollback_enabled           !rollback_enabled
                        │                          │
                        ↓                          ↓
                 WaitingForOldIp              NewIpTimeout
                        │                          │
                   (old IP responds)         (user clicks button)
                        │                          │
                        ↓                          ↓
                      Idle                    (manual nav)

*/
