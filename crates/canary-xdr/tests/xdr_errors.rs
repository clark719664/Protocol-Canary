//! Unit tests covering XDR decoder and encoder error paths on empty,
//! invalid-base64, truncated, corrupted, and trailing-byte payloads.

use canary_xdr::{
    decode, decode_ledger_entry, decode_transaction_envelope, encode, DecodedValue, XdrDecodeError,
    XdrError, XdrTypeName,
};

#[test]
fn rejects_empty_and_zero_byte_xdr_strings_without_panicking() {
    let err_env = decode_transaction_envelope("").unwrap_err();
    match err_env {
        XdrDecodeError::InvalidXdr {
            type_name, offset, ..
        } => {
            assert_eq!(type_name, XdrTypeName::TransactionEnvelope);
            assert_eq!(offset, 0);
        }
        other => panic!("expected InvalidXdr at offset 0, got {other:?}"),
    }

    let err_le = decode_ledger_entry("   \n\t  ").unwrap_err();
    match err_le {
        XdrDecodeError::InvalidXdr {
            type_name, offset, ..
        } => {
            assert_eq!(type_name, XdrTypeName::LedgerEntry);
            assert_eq!(offset, 0);
        }
        other => panic!("expected InvalidXdr at offset 0, got {other:?}"),
    }

    for ty in [
        XdrTypeName::StellarValue,
        XdrTypeName::ContractExecutable,
        XdrTypeName::ScVal,
        XdrTypeName::TransactionEnvelope,
        XdrTypeName::LedgerEntry,
    ] {
        assert!(decode(ty, "").is_err());
    }
}

#[test]
fn rejects_invalid_base64_and_bad_padding_with_typed_decode_error() {
    for bad_b64 in ["!!!not-base64!!!", "AAAA===", "AA", "AQIDBAUGB==="] {
        let env_err = decode_transaction_envelope(bad_b64).unwrap_err();
        assert!(
            matches!(env_err, XdrDecodeError::InvalidBase64(_)),
            "expected InvalidBase64 for {bad_b64:?}, got {env_err:?}"
        );
        assert_eq!(env_err.byte_offset(), 0);

        let le_err = decode_ledger_entry(bad_b64).unwrap_err();
        assert!(
            matches!(le_err, XdrDecodeError::InvalidBase64(_)),
            "expected InvalidBase64 for {bad_b64:?}, got {le_err:?}"
        );
        assert_eq!(le_err.byte_offset(), 0);

        assert!(
            decode(XdrTypeName::ScVal, bad_b64).is_err(),
            "expected decode error for {bad_b64:?}"
        );
    }

    let fixture_err = XdrError::InvalidFixtureBody {
        source_path: "xdr/bad.toml".into(),
        reason: "missing type".into(),
    };
    assert!(fixture_err.to_string().contains("xdr/bad.toml"));
}

#[test]
fn rejects_truncated_corrupted_and_trailing_byte_xdr_payloads() {
    // 2 bytes (`AA==` -> `[0, 0]`) is shorter than a 4-byte XDR discriminant.
    let truncated = decode_transaction_envelope("AA==").unwrap_err();
    match &truncated {
        XdrDecodeError::InvalidXdr {
            type_name, offset, ..
        } => {
            assert_eq!(*type_name, XdrTypeName::TransactionEnvelope);
            assert!(*offset <= 2);
        }
        other => panic!("expected InvalidXdr on truncated payload, got {other:?}"),
    }

    // Invalid discriminant `0x7f7f7f7f` (`f39/fw==`).
    let corrupted_disc = decode_ledger_entry("f39/fw==").unwrap_err();
    match &corrupted_disc {
        XdrDecodeError::InvalidXdr { type_name, .. } => {
            assert_eq!(*type_name, XdrTypeName::LedgerEntry);
        }
        other => panic!("expected InvalidXdr on corrupted discriminant, got {other:?}"),
    }

    // Valid `TransactionEnvelope::default()` followed by 4 trailing zero bytes.
    let valid_env = stellar_xdr::TransactionEnvelope::default();
    let valid_b64 =
        encode(&DecodedValue::TransactionEnvelope(Box::new(valid_env))).expect("encodes");
    let mut raw_bytes =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &valid_b64).unwrap();
    let valid_len = raw_bytes.len() as u64;
    raw_bytes.extend_from_slice(&[0, 0, 0, 0]);
    let trailing_b64 =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &raw_bytes);

    let trailing_err = decode_transaction_envelope(&trailing_b64).unwrap_err();
    match &trailing_err {
        XdrDecodeError::InvalidXdr {
            type_name,
            offset,
            message,
        } => {
            assert_eq!(*type_name, XdrTypeName::TransactionEnvelope);
            assert_eq!(*offset, valid_len);
            assert!(
                message.contains("trailing byte"),
                "expected trailing byte diagnostic, got: {message}"
            );
        }
        other => panic!("expected InvalidXdr for trailing bytes, got {other:?}"),
    }
}

#[test]
fn xdr_type_name_display_and_from_str_reject_unknown_types() {
    for (ty, name) in [
        (XdrTypeName::StellarValue, "StellarValue"),
        (XdrTypeName::ContractExecutable, "ContractExecutable"),
        (XdrTypeName::ScVal, "ScVal"),
        (XdrTypeName::TransactionEnvelope, "TransactionEnvelope"),
        (XdrTypeName::LedgerEntry, "LedgerEntry"),
    ] {
        assert_eq!(ty.to_string(), name);
        assert_eq!(name.parse::<XdrTypeName>().unwrap(), ty);
    }

    let err = "UnknownXdrType".parse::<XdrTypeName>().unwrap_err();
    assert!(err.contains("unsupported XDR type name"));
}
