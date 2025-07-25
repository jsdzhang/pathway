// Copyright © 2024 Pathway

use super::helpers::{
    create_persistence_manager, full_cycle_read, new_csv_filesystem_reader, new_filesystem_reader,
    FullReadResult,
};

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use tempfile::tempdir;

use pathway_engine::connectors::data_format::{
    DsvParser, DsvSettings, InnerSchemaField, JsonLinesParser, ParsedEvent, Parser,
};
use pathway_engine::connectors::data_storage::ReaderBuilder;
use pathway_engine::connectors::data_storage::{ConnectorMode, ReadMethod};
use pathway_engine::connectors::SessionType;
use pathway_engine::engine::{Result, Type, Value};
use pathway_engine::persistence::tracker::WorkerPersistentStorage;
use pathway_engine::persistence::PersistentId;

enum TestedFormat {
    Csv,
    Json,
}

fn csv_reader_parser_pair(input_path: &str) -> Result<(Box<dyn ReaderBuilder>, Box<dyn Parser>)> {
    let mut builder = csv::ReaderBuilder::new();
    builder.has_headers(false);
    let reader =
        new_csv_filesystem_reader(input_path, builder, ConnectorMode::Static, "*", true).unwrap();
    let schema = [
        ("key".to_string(), InnerSchemaField::new(Type::String, None)),
        (
            "value".to_string(),
            InnerSchemaField::new(Type::String, None),
        ),
    ];
    let parser = DsvParser::new(
        DsvSettings::new(
            Some(vec!["key".to_string()]),
            vec!["value".to_string()],
            ',',
        ),
        schema.into(),
    )?;
    Ok((Box::new(reader), Box::new(parser)))
}

fn json_reader_parser_pair(input_path: &str) -> Result<(Box<dyn ReaderBuilder>, Box<dyn Parser>)> {
    let reader = new_filesystem_reader(
        input_path,
        ConnectorMode::Static,
        ReadMethod::ByLine,
        "*",
        true,
    )
    .unwrap();
    let schema = [
        ("key".to_string(), InnerSchemaField::new(Type::Int, None)),
        (
            "value".to_string(),
            InnerSchemaField::new(Type::String, None),
        ),
    ];
    let parser = JsonLinesParser::new(
        Some(vec!["key".to_string()]),
        vec!["value".to_string()],
        HashMap::new(),
        true,
        schema.into(),
        SessionType::Native,
        None,
    )?;
    Ok((Box::new(reader), Box::new(parser)))
}

fn full_cycle_read_kv(
    format: TestedFormat,
    input_path: &Path,
    persistent_storage: Option<&Arc<Mutex<WorkerPersistentStorage>>>,
    persistent_id: Option<PersistentId>,
) -> Result<FullReadResult> {
    let (reader, mut parser) = match format {
        TestedFormat::Csv => csv_reader_parser_pair(input_path.to_str().unwrap())?,
        TestedFormat::Json => json_reader_parser_pair(input_path.to_str().unwrap())?,
    };
    Ok(full_cycle_read(
        reader,
        parser.as_mut(),
        persistent_storage,
        persistent_id,
    ))
}

#[test]
fn test_csv_file_recovery() -> eyre::Result<()> {
    let test_storage = tempdir()?;
    let test_storage_path = test_storage.path();

    let pstorage_root_path = test_storage_path.join("pstorage");
    let input_path = test_storage_path.join("input.csv");

    std::fs::write(&input_path, "key,value\n1,2\na,b").unwrap();
    {
        let tracker = create_persistence_manager(&pstorage_root_path, true);
        let data_stream =
            full_cycle_read_kv(TestedFormat::Csv, &input_path, Some(&tracker), Some(1))?;
        assert_eq!(
            data_stream.new_parsed_entries,
            vec![
                ParsedEvent::Insert((
                    Some(vec![Value::String("1".into())]),
                    vec![Value::String("2".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("a".into())]),
                    vec![Value::String("b".into())]
                ))
            ]
        );
    }

    std::fs::write(&input_path, "key,value\n1,2\na,b\nc,d\n55,66").unwrap();
    {
        let tracker = create_persistence_manager(&pstorage_root_path, false);
        let data_stream =
            full_cycle_read_kv(TestedFormat::Csv, &input_path, Some(&tracker), Some(1))?;
        eprintln!("data stream after: {:?}", data_stream.new_parsed_entries);
        assert_eq!(
            data_stream.new_parsed_entries,
            vec![
                ParsedEvent::Delete((
                    Some(vec![Value::String("1".into())]),
                    vec![Value::String("2".into())]
                )),
                ParsedEvent::Delete((
                    Some(vec![Value::String("a".into())]),
                    vec![Value::String("b".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("1".into())]),
                    vec![Value::String("2".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("a".into())]),
                    vec![Value::String("b".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("c".into())]),
                    vec![Value::String("d".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("55".into())]),
                    vec![Value::String("66".into())]
                ))
            ]
        );
    }

    Ok(())
}

#[test]
fn test_csv_dir_recovery() -> eyre::Result<()> {
    let test_storage = tempdir()?;
    let test_storage_path = test_storage.path();

    let pstorage_root_path = test_storage_path.join("pstorage");
    let inputs_dir_path = test_storage_path.join("inputs");
    std::fs::create_dir(&inputs_dir_path).unwrap_or(());

    std::fs::write(inputs_dir_path.join("input1.csv"), "key,value\n1,2\na,b").unwrap();
    std::fs::write(
        inputs_dir_path.join("input2.csv"),
        "key,value\nq,w\ne,r\nt,y",
    )
    .unwrap();

    {
        let tracker = create_persistence_manager(&pstorage_root_path, true);
        let data_stream =
            full_cycle_read_kv(TestedFormat::Csv, &inputs_dir_path, Some(&tracker), Some(1))?;
        assert_eq!(
            data_stream.new_parsed_entries,
            vec![
                ParsedEvent::Insert((
                    Some(vec![Value::String("1".into())]),
                    vec![Value::String("2".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("a".into())]),
                    vec![Value::String("b".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("q".into())]),
                    vec![Value::String("w".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("e".into())]),
                    vec![Value::String("r".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("t".into())]),
                    vec![Value::String("y".into())]
                )),
            ]
        );
    }

    //    std::fs::remove_file(inputs_dir_path.join("input1.csv")).unwrap();
    std::fs::write(
        inputs_dir_path.join("input2.csv"),
        "key,value\nq,w\ne,r\nt,y\np,q",
    )
    .unwrap();
    {
        let tracker = create_persistence_manager(&pstorage_root_path, false);
        let data_stream =
            full_cycle_read_kv(TestedFormat::Csv, &inputs_dir_path, Some(&tracker), Some(1))?;
        assert_eq!(
            data_stream.new_parsed_entries,
            vec![
                ParsedEvent::Delete((
                    Some(vec![Value::String("q".into())]),
                    vec![Value::String("w".into())]
                )),
                ParsedEvent::Delete((
                    Some(vec![Value::String("e".into())]),
                    vec![Value::String("r".into())]
                )),
                ParsedEvent::Delete((
                    Some(vec![Value::String("t".into())]),
                    vec![Value::String("y".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("q".into())]),
                    vec![Value::String("w".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("e".into())]),
                    vec![Value::String("r".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("t".into())]),
                    vec![Value::String("y".into())]
                )),
                ParsedEvent::Insert((
                    Some(vec![Value::String("p".into())]),
                    vec![Value::String("q".into())]
                ))
            ]
        );
    }

    Ok(())
}

#[test]
fn test_json_file_recovery() -> eyre::Result<()> {
    let test_storage = tempdir()?;
    let test_storage_path = test_storage.path();

    let pstorage_root_path = test_storage_path.join("pstorage");
    let input_path = test_storage_path.join("input.json");

    std::fs::write(
        &input_path,
        r#"{"key": 1, "value": "a"}
           {"key": 2, "value": "b"}"#,
    )
    .unwrap();
    {
        let tracker = create_persistence_manager(&pstorage_root_path, true);
        let data_stream =
            full_cycle_read_kv(TestedFormat::Json, &input_path, Some(&tracker), Some(1))?;
        assert_eq!(
            data_stream.new_parsed_entries,
            vec![
                ParsedEvent::Insert((Some(vec![Value::Int(1)]), vec![Value::String("a".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(2)]), vec![Value::String("b".into())]))
            ]
        );
    }

    std::fs::write(
        &input_path,
        r#"{"key": 1, "value": "a"}
           {"key": 2, "value": "b"}
           {"key": 3, "value": "c"}"#,
    )
    .unwrap();
    {
        let tracker = create_persistence_manager(&pstorage_root_path, false);
        let data_stream =
            full_cycle_read_kv(TestedFormat::Json, &input_path, Some(&tracker), Some(1))?;
        assert_eq!(
            data_stream.new_parsed_entries,
            vec![
                ParsedEvent::Delete((Some(vec![Value::Int(1)]), vec![Value::String("a".into())])),
                ParsedEvent::Delete((Some(vec![Value::Int(2)]), vec![Value::String("b".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(1)]), vec![Value::String("a".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(2)]), vec![Value::String("b".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(3)]), vec![Value::String("c".into())])),
            ]
        );
    }

    Ok(())
}

#[test]
fn test_json_folder_recovery() -> eyre::Result<()> {
    let test_storage = tempdir()?;
    let test_storage_path = test_storage.path();

    let pstorage_root_path = test_storage_path.join("pstorage");
    let inputs_dir_path = test_storage_path.join("inputs");
    std::fs::create_dir(&inputs_dir_path).unwrap_or(());

    std::fs::write(
        inputs_dir_path.as_path().join("input1.json"),
        r#"{"key": 1, "value": "a"}
           {"key": 2, "value": "b"}"#,
    )
    .unwrap();
    std::fs::write(
        inputs_dir_path.as_path().join("input2.json"),
        r#"{"key": 3, "value": "c"}
           {"key": 4, "value": "d"}"#,
    )
    .unwrap();
    {
        let tracker = create_persistence_manager(&pstorage_root_path, true);
        let data_stream = full_cycle_read_kv(
            TestedFormat::Json,
            &inputs_dir_path,
            Some(&tracker),
            Some(1),
        )?;
        assert_eq!(
            data_stream.new_parsed_entries,
            vec![
                ParsedEvent::Insert((Some(vec![Value::Int(1)]), vec![Value::String("a".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(2)]), vec![Value::String("b".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(3)]), vec![Value::String("c".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(4)]), vec![Value::String("d".into())]))
            ]
        );
    }
    std::fs::write(
        inputs_dir_path.as_path().join("input2.json"),
        r#"{"key": 3, "value": "c"}
           {"key": 4, "value": "d"}
           {"key": 5, "value": "e"}"#,
    )
    .unwrap();
    std::fs::write(
        inputs_dir_path.as_path().join("input3.json"),
        r#"{"key": 6, "value": "f"}
           {"key": 7, "value": "g"}"#,
    )
    .unwrap();
    {
        let tracker = create_persistence_manager(&pstorage_root_path, false);
        let data_stream = full_cycle_read_kv(
            TestedFormat::Json,
            &inputs_dir_path,
            Some(&tracker),
            Some(1),
        )?;
        assert_eq!(
            data_stream.new_parsed_entries,
            vec![
                ParsedEvent::Delete((Some(vec![Value::Int(3)]), vec![Value::String("c".into())])),
                ParsedEvent::Delete((Some(vec![Value::Int(4)]), vec![Value::String("d".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(3)]), vec![Value::String("c".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(4)]), vec![Value::String("d".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(5)]), vec![Value::String("e".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(6)]), vec![Value::String("f".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(7)]), vec![Value::String("g".into())])),
            ]
        );
    }

    Ok(())
}

#[test]
fn test_json_recovery_with_new_file() -> eyre::Result<()> {
    let test_storage = tempdir()?;
    let test_storage_path = test_storage.path();

    let pstorage_root_path = test_storage_path.join("pstorage");
    let inputs_dir_path = test_storage_path.join("inputs");
    std::fs::create_dir(&inputs_dir_path).unwrap_or(());

    std::fs::write(
        inputs_dir_path.as_path().join("input1.json"),
        r#"{"key": 1, "value": "a"}
           {"key": 2, "value": "b"}"#,
    )
    .unwrap();
    std::fs::write(
        inputs_dir_path.as_path().join("input2.json"),
        r#"{"key": 3, "value": "c"}
           {"key": 4, "value": "d"}"#,
    )
    .unwrap();
    {
        let tracker = create_persistence_manager(&pstorage_root_path, true);
        let data_stream = full_cycle_read_kv(
            TestedFormat::Json,
            &inputs_dir_path,
            Some(&tracker),
            Some(1),
        )?;
        assert_eq!(
            data_stream.new_parsed_entries,
            vec![
                ParsedEvent::Insert((Some(vec![Value::Int(1)]), vec![Value::String("a".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(2)]), vec![Value::String("b".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(3)]), vec![Value::String("c".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(4)]), vec![Value::String("d".into())]))
            ]
        );
    }

    std::fs::write(
        inputs_dir_path.as_path().join("input3.json"),
        r#"{"key": 5, "value": "e"}
           {"key": 6, "value": "f"}
           {"key": 7, "value": "g"}"#,
    )
    .unwrap();
    {
        let tracker = create_persistence_manager(&pstorage_root_path, false);
        let data_stream = full_cycle_read_kv(
            TestedFormat::Json,
            &inputs_dir_path,
            Some(&tracker),
            Some(1),
        )?;
        assert_eq!(
            data_stream.new_parsed_entries,
            vec![
                ParsedEvent::Insert((Some(vec![Value::Int(5)]), vec![Value::String("e".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(6)]), vec![Value::String("f".into())])),
                ParsedEvent::Insert((Some(vec![Value::Int(7)]), vec![Value::String("g".into())])),
            ]
        );
    }

    Ok(())
}
