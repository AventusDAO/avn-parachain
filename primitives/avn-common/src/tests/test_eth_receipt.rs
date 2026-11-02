use super::*;
use codec::Encode;
use hex_literal::hex;

const BRIDGE: H160 = H160(hex!("604dd282e3fbe35f40f84405f90965821483827f"));
const SIGNATURE: H256 =
    H256(hex!("39369dea7465bf87d004db7942da5e8e7fdf484fa950ea3d73cd75fb517b6416"));
const TOPIC_1: [u8; 32] = hex!("00000000000000000000000023aaf097c241897060c0a6b8aae61af5ea48cea3");
const DATA: [u8; 32] = hex!("0000000000000000000000000000000000000000000000000000000005f5e100");

fn log_json(address: &str, topics: &[&str], data: &str) -> String {
    let topics = topics.iter().map(|t| format!("\"{t}\"")).collect::<Vec<_>>().join(",");
    format!(
        r#"{{
            "logIndex": "0x0",
            "blockNumber": "0x2e",
            "address": "{address}",
            "topics": [{topics}],
            "data": "{data}",
            "removed": false
        }}"#
    )
}

fn receipt_json(status: &str, logs: &[String]) -> String {
    format!(
        r#"{{
            "transactionHash": "0x9ad4d46054b0495fa38e8418263c6107ecb4ffd879675372613edf39af898dcb",
            "blockNumber": "0x2e",
            "contractAddress": null,
            "logs": [{}],
            {status}
            "gasUsed": "0x6a4b"
        }}"#,
        logs.join(",")
    )
}

fn bridge_log() -> String {
    log_json(
        "0x604dd282e3fbe35f40f84405f90965821483827f",
        &[
            "0x39369dea7465bf87d004db7942da5e8e7fdf484fa950ea3d73cd75fb517b6416",
            "0x00000000000000000000000023aaf097c241897060c0a6b8aae61af5ea48cea3",
        ],
        "0x0000000000000000000000000000000000000000000000000000000005f5e100",
    )
}

fn expected_bridge_log() -> EthLog {
    EthLog {
        address: BRIDGE,
        topics: vec![SIGNATURE.0.to_vec(), TOPIC_1.to_vec()],
        data: DATA.to_vec(),
    }
}

fn query_response(receipt: &str, num_confirmations: u64) -> Vec<u8> {
    hex::encode(
        EthQueryResponse { data: receipt.as_bytes().to_vec().encode(), num_confirmations }.encode(),
    )
    .into_bytes()
}

mod parse_receipt {
    use super::*;

    #[test]
    fn reads_status_and_logs() {
        let json = receipt_json(r#""status": "0x1","#, &[bridge_log()]);

        let receipt = parse_receipt(json.as_bytes()).unwrap();

        assert!(receipt.succeeded);
        assert_eq!(receipt.logs, vec![expected_bridge_log()]);
    }

    #[test]
    fn reads_uppercase_hex_and_prefixes() {
        let log = log_json(
            "0X604DD282E3FBE35F40F84405F90965821483827F",
            &[
                "0X39369DEA7465BF87D004DB7942DA5E8E7FDF484FA950EA3D73CD75FB517B6416",
                "0X00000000000000000000000023AAF097C241897060C0A6B8AAE61AF5EA48CEA3",
            ],
            "0X0000000000000000000000000000000000000000000000000000000005F5E100",
        );
        let json = receipt_json(r#""status": "0X1","#, &[log]);

        let receipt = parse_receipt(json.as_bytes()).unwrap();

        assert!(receipt.succeeded);
        assert_eq!(receipt.logs, vec![expected_bridge_log()]);
    }

    #[test]
    fn reads_hex_without_prefix() {
        let log = log_json(
            "604dd282e3fbe35f40f84405f90965821483827f",
            &[
                "39369dea7465bf87d004db7942da5e8e7fdf484fa950ea3d73cd75fb517b6416",
                "00000000000000000000000023aaf097c241897060c0a6b8aae61af5ea48cea3",
            ],
            "0000000000000000000000000000000000000000000000000000000005f5e100",
        );
        let json = receipt_json(r#""status": "1","#, &[log]);

        let receipt = parse_receipt(json.as_bytes()).unwrap();

        assert!(receipt.succeeded);
        assert_eq!(receipt.logs, vec![expected_bridge_log()]);
    }

    #[test]
    fn reads_receipt_without_logs() {
        let json = receipt_json(r#""status": "0x1","#, &[]);

        assert_eq!(parse_receipt(json.as_bytes()).unwrap().logs, vec![]);
    }

    #[test]
    fn failed_transaction_did_not_succeed() {
        let json = receipt_json(r#""status": "0x0","#, &[bridge_log()]);

        assert!(!parse_receipt(json.as_bytes()).unwrap().succeeded);
    }

    #[test]
    fn receipt_without_status_did_not_succeed() {
        for status in ["", r#""status": null,"#] {
            let json = receipt_json(status, &[bridge_log()]);

            assert!(!parse_receipt(json.as_bytes()).unwrap().succeeded);
        }
    }

    #[test]
    fn empty_log_data_has_no_data() {
        let log = log_json("0x604dd282e3fbe35f40f84405f90965821483827f", &[], "0x");
        let json = receipt_json(r#""status": "0x1","#, &[log]);

        let receipt = parse_receipt(json.as_bytes()).unwrap();

        assert_eq!(receipt.logs[0].data(), None);
        assert_eq!(expected_bridge_log().data(), Some(DATA.to_vec()));
    }

    #[test]
    fn rejects_invalid_json() {
        for json in ["", "bad", "[]", "\"receipt\""] {
            assert_eq!(parse_receipt(json.as_bytes()), Err(ReceiptError::InvalidJson));
        }
    }

    #[test]
    fn rejects_invalid_status() {
        for status in [r#""status": "0xblah","#, r#""status": 1,"#] {
            let json = receipt_json(status, &[]);

            assert_eq!(parse_receipt(json.as_bytes()), Err(ReceiptError::InvalidField("status")));
        }
    }

    #[test]
    fn rejects_missing_logs() {
        for json in ["{}", r#"{"status": "0x1", "logs": null}"#, r#"{"logs": "0x"}"#] {
            assert_eq!(parse_receipt(json.as_bytes()), Err(ReceiptError::InvalidField("logs")));
        }
    }

    #[test]
    fn rejects_malformed_log() {
        let address = "0x604dd282e3fbe35f40f84405f90965821483827f";
        let topic = "0x39369dea7465bf87d004db7942da5e8e7fdf484fa950ea3d73cd75fb517b6416";
        let cases = [
            ("null".to_string(), "logs"),
            (log_json("0x", &[topic], "0x"), "address"),
            (log_json("0x604dd282", &[topic], "0x"), "address"),
            (log_json(address, &["0xblah"], "0x"), "topics"),
            (log_json(address, &[topic], "0xblah"), "data"),
            (log_json(address, &[topic], "0x123"), "data"),
            (format!(r#"{{"address": null, "topics": ["{topic}"], "data": "0x"}}"#), "address"),
            (format!(r#"{{"address": "{address}", "topics": null, "data": "0x"}}"#), "topics"),
            (format!(r#"{{"address": "{address}", "topics": ["{topic}"]}}"#), "data"),
        ];

        for (log, field) in cases {
            let json = receipt_json(r#""status": "0x1","#, &[log]);

            assert_eq!(parse_receipt(json.as_bytes()), Err(ReceiptError::InvalidField(field)));
        }
    }
}

mod find_logs {
    use super::*;

    fn receipt() -> EthReceipt {
        let other = EthLog { address: H160::repeat_byte(1), ..expected_bridge_log() };
        let unrelated = EthLog { topics: vec![TOPIC_1.to_vec()], ..expected_bridge_log() };
        let no_topics = EthLog { topics: vec![], ..expected_bridge_log() };
        EthReceipt {
            succeeded: true,
            logs: vec![no_topics, unrelated, other, expected_bridge_log()],
        }
    }

    #[test]
    fn find_log_returns_the_first_log_with_the_signature() {
        let receipt = receipt();

        assert_eq!(receipt.find_log(&SIGNATURE), Some(&receipt.logs[2]));
        assert_eq!(receipt.find_log(&H256::repeat_byte(1)), None);
    }

    #[test]
    fn logs_from_filters_by_address_and_signature() {
        let receipt = receipt();

        assert_eq!(
            receipt.logs_from(&BRIDGE, &SIGNATURE).collect::<Vec<_>>(),
            vec![&expected_bridge_log()]
        );
        assert_eq!(receipt.logs_from(&BRIDGE, &H256::repeat_byte(1)).count(), 0);
        assert_eq!(receipt.logs_from(&H160::repeat_byte(2), &SIGNATURE).count(), 0);
    }
}

mod decode_receipt_response {
    use super::*;

    #[test]
    fn decodes_receipt_and_confirmations() {
        let json = receipt_json(r#""status": "0x1","#, &[bridge_log()]);

        let (receipt, num_confirmations) =
            decode_receipt_response(&query_response(&json, 12)).unwrap().unwrap();

        assert!(receipt.succeeded);
        assert_eq!(receipt.logs, vec![expected_bridge_log()]);
        assert_eq!(num_confirmations, 12);
    }

    #[test]
    fn empty_response_has_no_receipt() {
        assert_eq!(decode_receipt_response(b""), Ok(None));
    }

    #[test]
    fn rejects_invalid_response() {
        let not_bytes =
            hex::encode(EthQueryResponse { data: vec![1], num_confirmations: 1 }.encode());

        for body in ["0xblah".as_bytes(), b"00", not_bytes.as_bytes()] {
            assert_eq!(decode_receipt_response(body), Err(ReceiptError::InvalidResponse));
        }
    }

    #[test]
    fn rejects_invalid_receipt() {
        assert_eq!(
            decode_receipt_response(&query_response("bad", 1)),
            Err(ReceiptError::InvalidJson)
        );
    }
}
