use crate::{event_parser::*, mock::*};
use hex_literal::hex;
use simple_json2::json::{JsonValue, NumberValue};
use sp_core::hash::H256;

struct MockEthEventsResponse {
    pub valid_events_response: Vec<(Vec<char>, JsonValue)>,
    pub valid_result_field: Vec<(Vec<char>, JsonValue)>,
    pub valid_event_1: Vec<(Vec<char>, JsonValue)>,
    pub valid_event_2: Vec<(Vec<char>, JsonValue)>,
    pub valid_topic_1: H256,
    pub valid_topic_2: H256,
    pub zero_topic: H256,
}

impl MockEthEventsResponse {
    fn setup() -> Self {
        let id_key: Vec<char> = "id".chars().collect();
        let id_num = NumberValue { integer: 1, exponent: 0, fraction: 0, fraction_length: 0 };
        let id_value = JsonValue::Number(id_num);
        let jsonrpc_key: Vec<char> = "jsonrpc".chars().collect();
        let jsonrpc_value = JsonValue::String("2.0".chars().collect());
        let data_key: Vec<char> = "data".chars().collect();
        let log_index_key: Vec<char> = "logIndex".chars().collect();
        let log_index_value_1 = JsonValue::String("0x0".chars().collect());
        let log_index_value_2 = JsonValue::String("0x1".chars().collect());
        let transaction_index_key: Vec<char> = "transactionIndex".chars().collect();
        let transaction_index_value_1 = JsonValue::String("0x0".chars().collect());
        let transaction_index_value_2 = JsonValue::String("0x1".chars().collect());
        let block_hash_key: Vec<char> = "blockHash".chars().collect();
        let block_hash_value = JsonValue::String(
            "0x5536c9e671fe581fe4ef4631112038297dcdecae163e8724c281ece8ad94c8c3".chars().collect(),
        );
        let transaction_hash_key: Vec<char> = "transactionHash".chars().collect();
        let transaction_hash_value = JsonValue::String(
            "0x9ad4d46054b0495fa38e8418263c6107ecb4ffd879675372613edf39af898dcb".chars().collect(),
        );
        let block_number_key: Vec<char> = "blockNumber".chars().collect();
        let block_number_value = JsonValue::String("0x2e".chars().collect());
        let address_key: Vec<char> = "address".chars().collect();
        let address_value_1 =
            JsonValue::String("0x604dd282e3fbe35f40f84405f90965821483827f".chars().collect());
        let address_value_2 =
            JsonValue::String("0X704DD282E3FBE35F40F84405F90965821483827F".chars().collect());
        let event_data_key: Vec<char> = "data".chars().collect();
        let data_value = JsonValue::String(
            "0xFF00000000000000000000000000000000000000000000000000000005F5e100".chars().collect(),
        );
        let topics_key: Vec<char> = "topics".chars().collect();
        let topics_value_1 = JsonValue::Array(vec![
            JsonValue::String(
                "0x39369dea7465bf87d004db7942da5e8e7fdf484fa950ea3d73cd75fb517b6416"
                    .chars()
                    .collect(),
            ),
            JsonValue::String(
                "0x00000000000000000000000023aaf097c241897060c0a6b8aae61af5ea48cea3"
                    .chars()
                    .collect(),
            ),
            JsonValue::String(
                "0x689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303"
                    .chars()
                    .collect(),
            ),
        ]);
        let topics_value_2 = JsonValue::Array(vec![
            JsonValue::String(
                "0X49369DEA7465BF87D004DB7942DA5E8E7FDF484FA950EA3D73CD75FB517B6416"
                    .chars()
                    .collect(),
            ),
            JsonValue::String(
                "0X10000000000000000000000023AAF097C241897060C0A6B8AAE61AF5EA48CEA3"
                    .chars()
                    .collect(),
            ),
            JsonValue::String(
                "0X789D5B000758030EA25304346869B002A345E7647EC5784B8AF986E24E971303"
                    .chars()
                    .collect(),
            ),
        ]);
        let type_key: Vec<char> = "type".chars().collect();
        let type_value = JsonValue::String("mined".chars().collect());
        let from_key: Vec<char> = "from".chars().collect();
        let from_value =
            JsonValue::String("0x3a629a342f842d2e548a372742babf288816da4e".chars().collect());
        let to_key: Vec<char> = "to".chars().collect();
        let to_value =
            JsonValue::String("0x604dd282e3fbe35f40f84405f90965821483827f".chars().collect());
        let gas_used_key: Vec<char> = "gasUsed".chars().collect();
        let gas_used_value = JsonValue::String("0x6a4b".chars().collect());
        let cumulative_gas_used_key: Vec<char> = "cumulativeGasUsed".chars().collect();
        let cumulative_gas_used_value = JsonValue::String("0x6a4b".chars().collect());
        let contract_address_key: Vec<char> = "contractAddress".chars().collect();
        let logs_key: Vec<char> = "logs".chars().collect();
        let status_key: Vec<char> = "status".chars().collect();
        let status_value = JsonValue::String("0x1".chars().collect());
        let logs_bloom_key: Vec<char> = "logsBloom".chars().collect();
        let logs_bloom_value =
            JsonValue::String("0x000001000000000000000000000000000".chars().collect());
        let v_key: Vec<char> = "v".chars().collect();
        let v_value = JsonValue::String("0x1c".chars().collect());
        let r_key: Vec<char> = "r".chars().collect();
        let r_value = JsonValue::String(
            "0x8823b54a06401fed57e03ac54b1a4cf81091dc1e44192b9a87ce4f4b9c56d454".chars().collect(),
        );
        let s_key: Vec<char> = "s".chars().collect();
        let s_value = JsonValue::String(
            "0x842e06a5258c4337148bc677f0b5ca343a8dfda597fb92f540ce443fd2bf340".chars().collect(),
        );

        let valid_event_1 = vec![
            (log_index_key.clone(), log_index_value_1),
            (transaction_index_key.clone(), transaction_index_value_1.clone()),
            (transaction_hash_key.clone(), transaction_hash_value.clone()),
            (block_hash_key.clone(), block_hash_value.clone()),
            (block_number_key.clone(), block_number_value.clone()),
            (address_key.clone(), address_value_1),
            (event_data_key.clone(), data_value.clone()),
            (topics_key.clone(), topics_value_1),
            (type_key.clone(), type_value.clone()),
        ];

        let valid_event_2 = vec![
            (log_index_key.clone(), log_index_value_2),
            (transaction_index_key.clone(), transaction_index_value_2),
            (transaction_hash_key.clone(), transaction_hash_value.clone()),
            (block_hash_key.clone(), block_hash_value.clone()),
            (block_number_key.clone(), block_number_value.clone()),
            (address_key.clone(), address_value_2),
            (event_data_key.clone(), data_value.clone()),
            (topics_key.clone(), topics_value_2),
            (type_key.clone(), type_value.clone()),
        ];

        let valid_result_field = vec![
            (transaction_hash_key.clone(), transaction_hash_value.clone()),
            (transaction_index_key.clone(), transaction_index_value_1.clone()),
            (block_hash_key.clone(), block_hash_value.clone()),
            (block_number_key.clone(), block_number_value.clone()),
            (from_key.clone(), from_value),
            (to_key.clone(), to_value),
            (gas_used_key.clone(), gas_used_value),
            (cumulative_gas_used_key.clone(), cumulative_gas_used_value),
            (contract_address_key.clone(), JsonValue::Null),
            (
                logs_key,
                JsonValue::Array(vec![
                    JsonValue::Object(valid_event_1.clone()),
                    JsonValue::Object(valid_event_2.clone()),
                ]),
            ),
            (status_key.clone(), status_value),
            (logs_bloom_key.clone(), logs_bloom_value),
            (v_key.clone(), v_value),
            (r_key.clone(), r_value),
            (s_key.clone(), s_value),
        ];

        let valid_events_response = vec![
            (id_key.clone(), id_value),
            (jsonrpc_key.clone(), jsonrpc_value),
            (data_key.clone(), JsonValue::Object(valid_result_field.clone())),
        ];

        MockEthEventsResponse {
            valid_events_response,
            valid_result_field,
            valid_event_1,
            valid_event_2,
            valid_topic_1: H256(hex!(
                "39369dea7465bf87d004db7942da5e8e7fdf484fa950ea3d73cd75fb517b6416"
            )),
            valid_topic_2: H256(hex!(
                "49369DEA7465BF87D004DB7942DA5E8E7FDF484FA950EA3D73CD75FB517B6416"
            )),
            zero_topic: H256::repeat_byte(0),
        }
    }
}

#[test]
fn get_events_should_return_expected_result_events_when_input_is_valid() {
    let mock_events_response = MockEthEventsResponse::setup();
    let events_result = get_events(&mock_events_response.valid_result_field);

    assert!(events_result.is_ok());
    assert_eq!(
        *events_result.unwrap(),
        vec![
            JsonValue::Object(mock_events_response.valid_event_1.clone()),
            JsonValue::Object(mock_events_response.valid_event_2.clone())
        ]
    );
}

#[test]
fn get_events_should_return_error_when_input_is_empty() {
    let s_key: Vec<char> = "s".chars().collect();
    let empty_event_response = JsonValue::Null;

    assert!(get_events(&vec![(s_key, empty_event_response)]).is_err());
}

#[test]
fn get_events_should_return_error_when_result_field_is_invalid() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut bad_events_response = mock_events_response.valid_events_response.clone();
    bad_events_response[INDEX_DATA].1 = JsonValue::Null;

    assert!(get_events(&bad_events_response).is_err());
}

#[test]
fn get_events_should_return_error_when_logs_field_is_invalid() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut bad_events_response = mock_events_response.valid_result_field.clone();
    bad_events_response[INDEX_RESULT_LOGS].1 = JsonValue::Object(bad_events_response.clone());

    assert!(get_events(&bad_events_response).is_err());
}

#[test]
fn get_status_should_return_expected_result_events_when_input_is_valid() {
    let mock_events_response = MockEthEventsResponse::setup();
    let status = get_status(&mock_events_response.valid_result_field);

    assert!(status.is_ok());
    assert_eq!(status.unwrap(), 1);
}

#[test]
fn get_status_should_return_error_when_input_is_empty() {
    let s_key: Vec<char> = "s".chars().collect();
    let empty_event_response = JsonValue::Null;

    assert!(get_status(&vec![(s_key, empty_event_response)]).is_err());
}

#[test]
fn get_status_should_return_error_when_data_field_is_invalid() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut bad_events_response = mock_events_response.valid_events_response.clone();
    bad_events_response[INDEX_DATA].1 = JsonValue::Null;

    assert!(get_status(&bad_events_response).is_err());
}

#[test]
fn get_status_should_return_error_when_status_field_is_invalid() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut bad_events_response = mock_events_response.valid_events_response.clone();
    let mut valid_result_field = mock_events_response.valid_result_field.clone();
    valid_result_field[INDEX_RESULT_STATUS].1 = JsonValue::String("invalid".chars().collect());
    bad_events_response[INDEX_DATA].1 = JsonValue::Object(valid_result_field);

    assert!(get_status(&bad_events_response).is_err());
}

#[test]
pub fn find_event_should_return_expected_result_event_when_event_values_are_in_lowercase() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let mock_events_response = MockEthEventsResponse::setup();
        let valid_topic_1 = mock_events_response.valid_topic_1;
        let (_, _, contract_address) =
            find_event(&mock_events_response.valid_result_field, valid_topic_1).unwrap();

        let mock_event = mock_events_response.valid_event_1.clone();
        assert_eq!(
            hex::encode(contract_address),
            mock_event[INDEX_EVENT_ADDRESS].1.get_string().unwrap().trim_start_matches("0x")
        );
    });
}

#[test]
pub fn find_event_should_return_expected_result_event_when_event_values_are_in_uppercase() {
    let mut ext = ExtBuilder::build_default().with_genesis_config().as_externality();
    ext.execute_with(|| {
        let mock_events_response = MockEthEventsResponse::setup();
        let valid_topic_2 = mock_events_response.valid_topic_2;
        let (_, _, contract_address) =
            find_event(&mock_events_response.valid_result_field, valid_topic_2).unwrap();

        let mock_event = mock_events_response.valid_event_2.clone();
        assert_eq!(
            hex::encode(contract_address).to_uppercase(),
            mock_event[INDEX_EVENT_ADDRESS].1.get_string().unwrap().trim_start_matches("0X")
        );
    });
}

#[test]
pub fn find_event_should_return_expected_result_event_when_event_topics_without_0x_prefix() {
    let mut mock_events_response = MockEthEventsResponse::setup();
    let mut without_0x_event_signature_topic_event = mock_events_response.valid_event_1.clone();
    without_0x_event_signature_topic_event[INDEX_TOPICS].1 = JsonValue::Array(vec![
        JsonValue::String(
            "39369dea7465bf87d004db7942da5e8e7fdf484fa950ea3d73cd75fb517b6416".chars().collect(),
        ),
        JsonValue::String(
            "00000000000000000000000023aaf097c241897060c0a6b8aae61af5ea48cea3".chars().collect(),
        ),
        JsonValue::String(
            "689d5b000758030ea25304346869b002a345e7647ec5784b8af986e24e971303".chars().collect(),
        ),
    ]);
    let valid_events = vec![
        JsonValue::Object(without_0x_event_signature_topic_event.clone()),
        JsonValue::Object(mock_events_response.valid_event_2.clone()),
    ];

    mock_events_response.valid_result_field[INDEX_RESULT_LOGS].1 = JsonValue::Array(valid_events);
    let valid_event_1 = mock_events_response.valid_event_1.clone();
    let (_, _, contract_address) =
        find_event(&mock_events_response.valid_result_field, mock_events_response.valid_topic_1)
            .unwrap();

    assert_eq!(
        hex::encode(contract_address),
        valid_event_1[INDEX_EVENT_ADDRESS].1.get_string().unwrap().trim_start_matches("0x")
    );
}

#[test]
pub fn find_event_should_return_none_when_topic_not_match_in_events() {
    let mock_events_response = MockEthEventsResponse::setup();
    let zero_topic = mock_events_response.zero_topic;

    assert!(find_event(&mock_events_response.valid_result_field, zero_topic).is_none());
}

#[test]
pub fn find_event_should_return_none_when_events_are_empty() {
    let mut mock_events_response = MockEthEventsResponse::setup();
    let empty_events = Vec::new();
    let zero_topic = mock_events_response.zero_topic;
    mock_events_response.valid_result_field[INDEX_RESULT_LOGS].1 = JsonValue::Array(empty_events);

    assert!(find_event(&mock_events_response.valid_result_field, zero_topic).is_none());
}

#[test]
pub fn find_event_should_return_none_when_input_events_are_misformatted() {
    let mut mock_events_response = MockEthEventsResponse::setup();
    let invalid_events = vec![JsonValue::Boolean(true), JsonValue::Boolean(false)];
    let valid_topic = mock_events_response.valid_topic_1;
    mock_events_response.valid_result_field[INDEX_RESULT_LOGS].1 = JsonValue::Array(invalid_events);
    assert!(find_event(&mock_events_response.valid_result_field, valid_topic).is_none());
}

#[test]
pub fn find_event_should_return_none_when_input_events_are_null() {
    let mut mock_events_response = MockEthEventsResponse::setup();
    let null_events = vec![JsonValue::Null];
    let valid_topic = mock_events_response.valid_topic_1;
    mock_events_response.valid_result_field[INDEX_RESULT_LOGS].1 = JsonValue::Array(null_events);
    assert!(find_event(&mock_events_response.valid_result_field, valid_topic).is_none());
}

#[test]
pub fn find_event_should_return_none_when_events_contains_null_contract_address() {
    let mut mock_events_response = MockEthEventsResponse::setup();
    let mut empty_address_event = mock_events_response.valid_event_1.clone();
    empty_address_event[INDEX_EVENT_ADDRESS].1 = JsonValue::Null;
    let invalid_events = vec![
        JsonValue::Object(empty_address_event),
        JsonValue::Object(mock_events_response.valid_event_2.clone()),
    ];
    let valid_topic = mock_events_response.valid_topic_1;
    mock_events_response.valid_result_field[INDEX_RESULT_LOGS].1 = JsonValue::Array(invalid_events);
    assert!(find_event(&mock_events_response.valid_result_field, valid_topic).is_none());
}

#[test]
pub fn find_event_should_return_none_when_event_topics_are_invalid_hex_string() {
    let mut mock_events_response = MockEthEventsResponse::setup();
    let mut invalid_hex_event_signature_topic_event = mock_events_response.valid_event_1.clone();
    invalid_hex_event_signature_topic_event[INDEX_TOPICS].1 =
        JsonValue::Array(vec![JsonValue::String(
            "0xggg69dea7465bf87d004db7942da5e8e7fdf484fa950ea3d73cd75fb517b6416".chars().collect(),
        )]);
    let invalid_events = vec![
        JsonValue::Object(invalid_hex_event_signature_topic_event),
        JsonValue::Object(mock_events_response.valid_event_2.clone()),
    ];
    let valid_topic = mock_events_response.valid_topic_1;
    mock_events_response.valid_result_field[INDEX_RESULT_LOGS].1 = JsonValue::Array(invalid_events);
    assert!(find_event(&mock_events_response.valid_result_field, valid_topic).is_none());
}

#[test]
pub fn find_event_should_return_none_when_event_topics_are_empty_strings() {
    let mut mock_events_response = MockEthEventsResponse::setup();
    let mut empty_signature_topic_event = mock_events_response.valid_event_1.clone();
    empty_signature_topic_event[INDEX_TOPICS].1 =
        JsonValue::Array(vec![JsonValue::String("".chars().collect())]);
    let invalid_events = vec![JsonValue::Object(empty_signature_topic_event)];
    let valid_topic = H256::repeat_byte(0);
    mock_events_response.valid_result_field[INDEX_RESULT_LOGS].1 = JsonValue::Array(invalid_events);
    assert!(find_event(&mock_events_response.valid_result_field, valid_topic).is_none());
}

#[test]
pub fn get_data_should_return_expected_result_when_input_is_valid() {
    let mock_events_response = MockEthEventsResponse::setup();
    let event_with_valid_data = JsonValue::Object(mock_events_response.valid_event_1.clone());

    let data_result = get_data(&event_with_valid_data);

    // Test characters are treated as a byte, and not as two ASCII characters
    assert_eq!(
        data_result.unwrap().unwrap(),
        vec![
            255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            5, 245, 225, 0
        ]
    );
}

#[test]
pub fn get_data_should_return_expected_result_when_event_data_without_0x_prefix() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut mock_event = mock_events_response.valid_event_1.clone();
    mock_event[INDEX_EVENT_DATA].1 = JsonValue::String(
        "FF00000000000000000000000000000000000000000000000000000005F5e100".chars().collect(),
    );
    let event_with_valid_data = JsonValue::Object(mock_event);

    let data_result = get_data(&event_with_valid_data);

    // Test characters are treated as a byte, and not as two ASCII characters
    assert_eq!(
        data_result.unwrap().unwrap(),
        vec![
            255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            5, 245, 225, 0
        ]
    );
}

#[test]
pub fn get_data_should_return_none_when_data_is_empty() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut mock_event = mock_events_response.valid_event_1.clone();
    mock_event[INDEX_EVENT_DATA].1 = JsonValue::String("".chars().collect());
    let event_without_data = JsonValue::Object(mock_event);

    let data_result = get_data(&event_without_data);

    assert_eq!(data_result.unwrap(), Option::None);
}

#[test]
pub fn get_data_should_return_error_when_event_data_is_null() {
    let null_event = JsonValue::Null;

    assert!(get_data(&null_event).is_err());
}

#[test]
pub fn get_data_should_return_error_when_event_data_is_invalid() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut mock_event = mock_events_response.valid_event_1.clone();
    mock_event[INDEX_EVENT_DATA].1 = JsonValue::Boolean(true);
    let event_with_invalid_data = JsonValue::Object(mock_event);

    assert!(get_data(&event_with_invalid_data).is_err());
}

#[test]
pub fn get_data_should_return_error_when_event_data_hex_string_contains_invalid_characters() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut mock_event = mock_events_response.valid_event_1.clone();
    mock_event[INDEX_EVENT_DATA].1 = JsonValue::String(
        "0xGGGGGGGGGGG00000000000000000000000000000000000000000000005F5e100".chars().collect(),
    );
    let event_with_invalid_data = JsonValue::Object(mock_event);

    assert!(get_data(&event_with_invalid_data).is_err());
}

#[test]
pub fn get_data_should_return_error_when_event_data_has_odd_length() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut mock_event = mock_events_response.valid_event_1.clone();
    mock_event[INDEX_EVENT_DATA].1 = JsonValue::String("0x0".chars().collect());
    let event_with_invalid_data = JsonValue::Object(mock_event);

    assert!(get_data(&event_with_invalid_data).is_err());
}

#[test]
pub fn get_topics_should_return_expected_result_when_input_is_valid() {
    let mock_events_response = MockEthEventsResponse::setup();
    let event_with_valid_topics = JsonValue::Object(mock_events_response.valid_event_1.clone());

    let topics_result = get_topics(&event_with_valid_topics);

    assert!(topics_result.is_ok());
    let topics = topics_result.unwrap();
    assert_eq!(topics.len(), 3);
    assert_eq!(
        topics,
        vec![
            vec![
                57, 54, 157, 234, 116, 101, 191, 135, 208, 4, 219, 121, 66, 218, 94, 142, 127, 223,
                72, 79, 169, 80, 234, 61, 115, 205, 117, 251, 81, 123, 100, 22
            ],
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 35, 170, 240, 151, 194, 65, 137, 112, 96, 192,
                166, 184, 170, 230, 26, 245, 234, 72, 206, 163
            ],
            vec![
                104, 157, 91, 0, 7, 88, 3, 14, 162, 83, 4, 52, 104, 105, 176, 2, 163, 69, 231, 100,
                126, 197, 120, 75, 138, 249, 134, 226, 78, 151, 19, 3
            ]
        ]
    );
}

#[test]
pub fn get_topics_should_return_empty_when_event_topics_are_empty() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut mock_event = mock_events_response.valid_event_1.clone();
    mock_event[INDEX_TOPICS].1 = JsonValue::Array(Vec::<JsonValue>::new());
    let event_without_topics = JsonValue::Object(mock_event);

    let topics_result = get_topics(&event_without_topics);

    assert!(topics_result.unwrap().is_empty());
}

#[test]
pub fn get_topics_should_return_error_when_topics_are_null() {
    let null_event = JsonValue::Null;

    assert!(get_data(&null_event).is_err());
}

#[test]
pub fn get_topics_should_return_error_when_topics_are_invalid() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut mock_event = mock_events_response.valid_event_1.clone();
    mock_event[INDEX_TOPICS].1 = JsonValue::Boolean(true);
    let event_with_invalid_topics = JsonValue::Object(mock_event);

    assert!(get_topics(&event_with_invalid_topics).is_err());
}

#[test]
pub fn get_topics_should_return_error_when_topics_contain_non_hex_characters() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut mock_event = mock_events_response.valid_event_1.clone();
    mock_event[INDEX_TOPICS].1 = JsonValue::Array(vec![JsonValue::String(
        "0xGGGGGGGGGGG00000000000000000000000000000000000000000000005F5e100".chars().collect(),
    )]);
    let event_with_invalid_topics = JsonValue::Object(mock_event);

    assert!(get_topics(&event_with_invalid_topics).is_err());
}

#[test]
pub fn get_topics_should_return_error_when_topics_have_odd_lengths() {
    let mock_events_response = MockEthEventsResponse::setup();
    let mut mock_event = mock_events_response.valid_event_1.clone();
    mock_event[INDEX_TOPICS].1 = JsonValue::Array(vec![JsonValue::String("0x0".chars().collect())]);
    let event_with_invalid_topics = JsonValue::Object(mock_event);

    assert!(get_topics(&event_with_invalid_topics).is_err());
}

#[test]
pub fn get_value_of_works() {
    let mock_events_response = MockEthEventsResponse::setup();
    let valid_events_response = mock_events_response.valid_events_response.clone();
    let valid_result_field = mock_events_response.valid_result_field.clone();
    let mock_event = mock_events_response.valid_event_1.clone();

    assert_eq!(
        get_value_of(String::from("data"), &valid_events_response).unwrap(),
        &valid_events_response[INDEX_DATA].1
    );

    assert_eq!(
        get_value_of(String::from("logs"), &valid_result_field).unwrap(),
        &valid_result_field[INDEX_RESULT_LOGS].1
    );

    assert_eq!(
        get_value_of(String::from("data"), &mock_event).unwrap(),
        &mock_event[INDEX_EVENT_DATA].1
    );

    assert_eq!(
        get_value_of(String::from("topics"), &mock_event).unwrap(),
        &mock_event[INDEX_TOPICS].1
    );

    assert_eq!(
        get_value_of(String::from("address"), &mock_event).unwrap(),
        &mock_event[INDEX_EVENT_ADDRESS].1
    );
}
