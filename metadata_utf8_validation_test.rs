#[cfg(test)]
mod metadata_utf8_validation_tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Env};
    use soroban_sdk::{Address, Env, String};
    use crate::test::TestEnv;
    use crate::types::Error;

    fn setup_test_subscription() -> (TestEnv, u32, Address, Address) {
        let test_env = TestEnv::default();
        let subscriber = Address::generate(&test_env.env);
        let merchant = Address::generate(&test_env.env);
        
        // Mint tokens for subscription creation
        test_env.stellar_token_client().mint(&subscriber, &10_000_000);
        
        // Create subscription
        let sub_id = test_env.client.create_subscription(
            &subscriber,
            &merchant,
            &1000,
            &86400,
            &true,
            &None::<i128>,
            &None::<u64>,
            &None::<u32>,
        );
        
        (test_env, sub_id, subscriber, merchant)
    }

    #[test]
    fn test_valid_utf8_metadata_accepted() {
        let (test_env, sub_id, subscriber, _merchant) = setup_test_subscription();
        
        // Test various valid UTF-8 strings
        let valid_cases = vec![
            ("basic", "value"),
            ("with-unicode", "héllo wørld 🚀"),
            ("numbers", "123456789"),
            ("mixed", "Key123-Value_αβγ"),
            ("spaces", "hello world"),
            ("special-chars", "!@#$%^&*()"),
        ];
        
        for (key, value) in valid_cases {
            let key_str = String::from_str(&test_env.env, key);
            let value_str = String::from_str(&test_env.env, value);
            
            // Should succeed
            let result = test_env.client.try_set_metadata(
                &subscriber, &sub_id, &key_str, &value_str
            );
            assert!(result.is_ok(), "Valid UTF-8 '{}' -> '{}' should be accepted", key, value);
            
            // Verify the metadata was stored
            let stored_value = test_env.client.get_metadata(&sub_id, &key_str);
            assert_eq!(stored_value, value_str);
        }
    }

    #[test] 
    fn test_empty_strings_rejected() {
        let (test_env, sub_id, subscriber, _merchant) = setup_test_subscription();
        
        // Empty key should be rejected
        let empty_key = String::from_str(&test_env.env, "");
        let valid_value = String::from_str(&test_env.env, "value");
        let result = test_env.client.try_set_metadata(
            &subscriber, &sub_id, &empty_key, &valid_value
        );
        assert_eq!(result, Err(Ok(Error::InvalidInput)));
        
        // Empty value should be rejected
        let valid_key = String::from_str(&test_env.env, "key");
        let empty_value = String::from_str(&test_env.env, "");
        let result = test_env.client.try_set_metadata(
            &subscriber, &sub_id, &valid_key, &empty_value
        );
        assert_eq!(result, Err(Ok(Error::InvalidInput)));
    }

    #[test]
    fn test_whitespace_only_strings_rejected() {
        let (test_env, sub_id, subscriber, _merchant) = setup_test_subscription();
        
        // Whitespace-only strings should be rejected
        let whitespace_cases = vec![
            " ",           // single space
            "   ",         // multiple spaces  
            "\t",          // tab only
            "\n",          // newline only
            "\r",          // carriage return only
            " \t\n\r ",    // mixed whitespace
        ];
        
        for whitespace in whitespace_cases {
            let key_str = String::from_str(&test_env.env, "key");
            let whitespace_str = String::from_str(&test_env.env, whitespace);
            
            let result = test_env.client.try_set_metadata(
                &subscriber, &sub_id, &key_str, &whitespace_str
            );
            assert_eq!(result, Err(Ok(Error::InvalidInput)), 
                      "Whitespace-only value '{}' should be rejected", whitespace);
        }
    }

    #[test]
    fn test_mixed_valid_content_accepted() {
        let (test_env, sub_id, subscriber, _merchant) = setup_test_subscription();
        
        // Strings with valid content mixed with whitespace should be accepted
        let valid_mixed_cases = vec![
            ("key", " value "),           // value with leading/trailing spaces
            ("key2", "hello world"),      // value with internal spaces
            ("key3", "line1\nline2"),     // value with newline
            ("key4", "col1\tcol2"),       // value with tab
        ];
        
        for (key, value) in valid_mixed_cases {
            let key_str = String::from_str(&test_env.env, key);
            let value_str = String::from_str(&test_env.env, value);
            
            let result = test_env.client.try_set_metadata(
                &subscriber, &sub_id, &key_str, &value_str
            );
            assert!(result.is_ok(), "Mixed content '{}' -> '{}' should be accepted", key, value);
        }
    }

    #[test]
    fn test_signed_metadata_utf8_validation() {
        let (test_env, sub_id, subscriber, _merchant) = setup_test_subscription();
        
        // Create a signed metadata payload with invalid UTF-8 value
        use soroban_sdk::{BytesN, crypto::Hash};
        use crate::types::SignedMetadataPayload;
        
        // Generate keypair for signing
        let secret_key = BytesN::<32>::from_array(&test_env.env, &[1u8; 32]);
        let public_key = test_env.env.crypto().ed25519_public_key_from_secret_key(&secret_key);
        
        // Create payload with empty value (should be rejected)
        let payload = SignedMetadataPayload {
            subscription_id: sub_id,
            key: String::from_str(&test_env.env, "test_key"),
            value: String::from_str(&test_env.env, ""), // Empty value
            nonce: 0,
            expires_at: test_env.env.ledger().timestamp() + 3600,
        };
        
        // Sign the payload (this part would normally create a proper signature)
        let dummy_signature = BytesN::<64>::from_array(&test_env.env, &[0u8; 64]);
        
        // Try to set signed metadata with invalid UTF-8 - this should fail at validation
        // Note: This test focuses on the validation logic rather than the full signing flow
        let result = crate::metadata::apply_metadata_value(
            &test_env.env, sub_id, &payload.key, &payload.value
        );
        assert_eq!(result, Err(Error::InvalidInput));
    }

    #[test]
    fn test_validation_preserves_existing_functionality() {
        let (test_env, sub_id, subscriber, merchant) = setup_test_subscription();
        
        // Ensure normal metadata operations still work
        let key = String::from_str(&test_env.env, "test_key");
        let value = String::from_str(&test_env.env, "test_value");
        
        // Set metadata as subscriber
        test_env.client.set_metadata(&subscriber, &sub_id, &key, &value);
        
        // Verify it was stored
        let stored = test_env.client.get_metadata(&sub_id, &key);
        assert_eq!(stored, value);
        
        // Set metadata as merchant
        let key2 = String::from_str(&test_env.env, "merchant_key");
        let value2 = String::from_str(&test_env.env, "merchant_value");
        test_env.client.set_metadata(&merchant, &sub_id, &key2, &value2);
        
        // Verify merchant metadata
        let stored2 = test_env.client.get_metadata(&sub_id, &key2);
        assert_eq!(stored2, value2);
        
        // List keys
        let keys = test_env.client.list_metadata_keys(&sub_id);
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&key));
        assert!(keys.contains(&key2));
        
        // Delete metadata
        test_env.client.delete_metadata(&subscriber, &sub_id, &key);
        let result = test_env.client.try_get_metadata(&sub_id, &key);
        assert_eq!(result, Err(Ok(Error::NotFound)));
    }

    #[test]
    fn test_length_limits_still_enforced() {
        let (test_env, sub_id, subscriber, _merchant) = setup_test_subscription();
        
        // Test that length limits are still enforced after UTF-8 validation
        let max_key_len = crate::types::MAX_METADATA_KEY_LENGTH as usize;
        let max_value_len = crate::types::MAX_METADATA_VALUE_LENGTH as usize;
        
        // Create strings that are too long
        let too_long_key = "a".repeat(max_key_len + 1);
        let too_long_value = "b".repeat(max_value_len + 1);
        
        let valid_key = String::from_str(&test_env.env, "key");
        let valid_value = String::from_str(&test_env.env, "value");
        let long_key_str = String::from_str(&test_env.env, &too_long_key);
        let long_value_str = String::from_str(&test_env.env, &too_long_value);
        
        // Too long key should fail
        let result = test_env.client.try_set_metadata(
            &subscriber, &sub_id, &long_key_str, &valid_value
        );
        assert_eq!(result, Err(Ok(Error::MetadataKeyTooLong)));
        
        // Too long value should fail  
        let result = test_env.client.try_set_metadata(
            &subscriber, &sub_id, &valid_key, &long_value_str
        );
        assert_eq!(result, Err(Ok(Error::MetadataValueTooLong)));
    }

    #[test] 
    fn test_event_emission_with_validated_metadata() {
        let (test_env, sub_id, subscriber, _merchant) = setup_test_subscription();
        
        let key = String::from_str(&test_env.env, "event_key");
        let value = String::from_str(&test_env.env, "event_value_with_🚀");
        
        // Set metadata - this should emit a MetadataSetEvent
        test_env.client.set_metadata(&subscriber, &sub_id, &key, &value);
        
        // Check events were emitted
        let events = test_env.env.events().all();
        let metadata_events: Vec<_> = events.iter()
            .filter(|e| e.topics.first().map(|t| t.to_string()) == Some("metadata_set".to_string()))
            .collect();
        
        assert!(!metadata_events.is_empty(), "MetadataSetEvent should be emitted");
        
        // The event should contain the UTF-8 validated key, which off-chain
        // indexers can safely parse as UTF-8
    }
}