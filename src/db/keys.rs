pub fn user_pk(user_id: &str) -> String {
    format!("USER#{}", user_id)
}

pub fn profile_sk() -> &'static str {
    "PROFILE"
}

pub fn device_sk(device_id: &str) -> String {
    format!("DEVICE#{}", device_id)
}

pub fn pending_reg_key(user_id: &str) -> String {
    format!("reg:pending:{}", user_id)
}

pub fn email_lookup_pk(email: &str) -> String {
    format!("EMAIL#{}", email.to_lowercase().trim())
}

pub fn phone_lookup_pk(phone: &str) -> String {
    format!("PHONE#{}", phone.trim())
}

pub fn lookup_sk() -> &'static str {
    "PTR"
}

pub fn reporter_pk(reporter_id: &str) -> String {
    format!("REPORTER#{}", reporter_id)
}

pub fn offline_message_pk(recipient_id: &str) -> String {
    format!("OFFLINE#{}", recipient_id)
}

pub fn offline_message_sk(sender_id: &str, timestamp_secs: u64) -> String {
    format!("{}#{}", sender_id, timestamp_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_pk() {
        assert_eq!(user_pk("user-123"), "USER#user-123");
    }

    #[test]
    fn test_reporter_pk() {
        assert_eq!(reporter_pk("reporter-789"), "REPORTER#reporter-789");
    }

    #[test]
    fn test_device_sk() {
        assert_eq!(device_sk("dev-456"), "DEVICE#dev-456");
    }

    #[test]
    fn test_pending_reg_key() {
        assert_eq!(pending_reg_key("user-123"), "reg:pending:user-123");
    }

    #[test]
    fn test_email_lookup_pk() {
        assert_eq!(
            email_lookup_pk(" Test.User@Example.COM "),
            "EMAIL#test.user@example.com"
        );
    }

    #[test]
    fn test_phone_lookup_pk() {
        assert_eq!(phone_lookup_pk("  +1234567890  "), "PHONE#+1234567890");
        assert_eq!(phone_lookup_pk("  +447123456789  "), "PHONE#+447123456789");
    }

    #[test]
    fn test_static_sks() {
        assert_eq!(profile_sk(), "PROFILE");
        assert_eq!(lookup_sk(), "PTR");
    }

    #[test]
    fn test_offline_message_keys() {
        assert_eq!(offline_message_pk("rec-123"), "OFFLINE#rec-123");
        assert_eq!(
            offline_message_sk("sen-456", 1700000000),
            "sen-456#1700000000"
        );
    }
}
