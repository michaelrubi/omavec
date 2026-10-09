//! Decoding `.fig` files with kiwi-schema.

use std::collections::HashMap;
use std::io::Read;

/// Why a `.fig` file could not be decoded.
#[derive(Debug, thiserror::Error)]
pub enum FigError {
    #[error("not a .fig file: {0}")]
    NotAFig(&'static str),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("decompression error: {0}")]
    Decompression(String),
    #[error("schema error: {0}")]
    Schema(String),
    #[error("message decode error: {0}")]
    Message(String),
    #[error("tree error: {0}")]
    Tree(String),
}

const ZIP_MAGIC: &[u8; 2] = b"PK";
const FIG_MAGIC: &[u8; 8] = b"fig-kiwi";
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

fn decompress_chunk(compressed: &[u8]) -> Result<Vec<u8>, FigError> {
    if compressed.starts_with(&ZSTD_MAGIC) {
        let mut decoder = ruzstd::decoding::StreamingDecoder::new(compressed)
            .map_err(|e| FigError::Decompression(format!("zstd init error: {e:?}")))?;
        let mut out = Vec::new();
        decoder.read_to_end(&mut out)
            .map_err(|e| FigError::Decompression(format!("zstd read error: {e}")))?;
        Ok(out)
    } else {
        let mut decoder = flate2::read::DeflateDecoder::new(compressed);
        let mut out = Vec::new();
        decoder.read_to_end(&mut out)
            .map_err(|e| FigError::Decompression(format!("deflate read error: {e}")))?;
        Ok(out)
    }
}

fn read_chunk<'a>(data: &'a [u8], offset: &mut usize) -> Result<&'a [u8], FigError> {
    let len_end = offset.checked_add(4)
        .ok_or(FigError::NotAFig("offset overflow reading chunk length"))?;
    let len_bytes = data.get(*offset..len_end)
        .ok_or(FigError::NotAFig("unexpected end of file reading chunk length"))?;
    let len = u32::from_le_bytes(
        len_bytes.try_into().map_err(|_| FigError::NotAFig("invalid chunk length"))?,
    ) as usize;
    *offset = len_end;
    let chunk_end = offset.checked_add(len)
        .ok_or(FigError::NotAFig("chunk length overflow"))?;
    let chunk = data.get(*offset..chunk_end)
        .ok_or(FigError::NotAFig("unexpected end of file reading chunk body"))?;
    *offset = chunk_end;
    Ok(chunk)
}

fn read_and_inflate_chunk(data: &[u8], offset: &mut usize) -> Result<Vec<u8>, FigError> {
    let chunk = read_chunk(data, offset)?;
    decompress_chunk(chunk)
}

pub fn decode(file: &[u8]) -> Result<serde_json::Value, FigError> {
    if file.len() < 2 {
        return Err(FigError::NotAFig("file too short"));
    }

    let fig_data = if file.starts_with(ZIP_MAGIC) {
        let cursor = std::io::Cursor::new(file);
        let mut archive = zip::ZipArchive::new(cursor)?;
        let mut entry = archive.by_name("canvas.fig")
            .map_err(|_| FigError::NotAFig("zip missing canvas.fig"))?;
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)
            .map_err(|e| FigError::Decompression(format!("failed reading canvas.fig: {e}")))?;
        buf
    } else {
        file.to_vec()
    };

    if fig_data.len() < 16 || !fig_data.starts_with(FIG_MAGIC) {
        return Err(FigError::NotAFig("invalid fig header"));
    }

    let version_bytes = fig_data.get(8..12)
        .ok_or(FigError::NotAFig("missing version in fig header"))?;
    let version = u32::from_le_bytes(
        version_bytes.try_into().map_err(|_| FigError::NotAFig("invalid version bytes"))?,
    );
    let mut offset = 12;

    let schema_bytes = read_and_inflate_chunk(&fig_data, &mut offset)?;
    let schema = kiwi_schema::Schema::decode(&schema_bytes)
        .map_err(|()| FigError::Schema("failed to decode kiwi schema".to_string()))?;

    let data_bytes = read_and_inflate_chunk(&fig_data, &mut offset)?;
    let message_def = schema.def("Message")
        .ok_or_else(|| FigError::Schema("schema missing Message definition".to_string()))?;

    let kiwi_val = kiwi_schema::Value::decode(&schema, message_def.index, &data_bytes)
        .map_err(|()| FigError::Message("failed to decode kiwi message".to_string()))?;

    let mut json_val = kiwi_to_json(&kiwi_val);
    if let serde_json::Value::Object(ref mut map) = json_val {
        map.insert("version".to_string(), serde_json::json!(version));
        map.insert("fig_version".to_string(), serde_json::json!(version));
        map.insert("schema_definitions".to_string(), serde_json::json!(schema.defs.len()));
        map.insert("schema_definition_count".to_string(), serde_json::json!(schema.defs.len()));
    }

    Ok(json_val)
}

fn kiwi_to_json(val: &kiwi_schema::Value) -> serde_json::Value {
    match val {
        kiwi_schema::Value::Bool(b) => serde_json::Value::Bool(*b),
        kiwi_schema::Value::Byte(b) => serde_json::Value::Number((*b).into()),
        kiwi_schema::Value::Int(i) => serde_json::Value::Number((*i).into()),
        kiwi_schema::Value::UInt(u) => serde_json::Value::Number((*u).into()),
        kiwi_schema::Value::Float(f) => {
            serde_json::Number::from_f64(*f as f64)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        }
        kiwi_schema::Value::String(s) => serde_json::Value::String(s.clone()),
        kiwi_schema::Value::Int64(i) => serde_json::Value::Number((*i).into()),
        kiwi_schema::Value::UInt64(u) => serde_json::Value::Number((*u).into()),
        kiwi_schema::Value::Enum(_, variant) => serde_json::Value::String(variant.to_string()),
        kiwi_schema::Value::Array(items) => {
            // Byte arrays in the message can be large (blobs, images); encode them
            // in the JSON as a string "<N bytes>" rather than an array of numbers.
            if items.first().is_some_and(|item| matches!(item, kiwi_schema::Value::Byte(_))) {
                serde_json::Value::String(format!("<{} bytes>", items.len()))
            } else {
                serde_json::Value::Array(items.iter().map(kiwi_to_json).collect())
            }
        }
        kiwi_schema::Value::Object(_, fields) => {
            let mut map = serde_json::Map::with_capacity(fields.len());
            for (k, v) in fields {
                map.insert(k.to_string(), kiwi_to_json(v));
            }
            serde_json::Value::Object(map)
        }
    }
}

pub fn tree(message: &serde_json::Value) -> Result<String, FigError> {
    let node_changes = message.get("nodeChanges")
        .and_then(|v| v.as_array())
        .ok_or_else(|| FigError::Tree("missing nodeChanges in message".to_string()))?;

    let mut nodes: HashMap<(u32, u32), &serde_json::Value> = HashMap::new();
    let mut children_map: HashMap<(u32, u32), Vec<&serde_json::Value>> = HashMap::new();
    let mut root_guid = None;

    for node in node_changes {
        let guid = parse_guid(node.get("guid"))?;
        if root_guid.is_none() {
            root_guid = Some(guid);
        }
        nodes.insert(guid, node);
        children_map.entry(guid).or_default();
    }

    let root = root_guid.ok_or_else(|| FigError::Tree("no nodes found in nodeChanges".to_string()))?;

    for node in nodes.values() {
        if let Some(parent_index) = node.get("parentIndex") {
            let parent_guid = parse_guid(parent_index.get("guid"))?;
            if let Some(children) = children_map.get_mut(&parent_guid) {
                children.push(node);
            }
        }
    }

    let mut lines = Vec::new();
    walk_tree(root, 0, &nodes, &children_map, &mut lines)?;

    let mut out = lines.join("\n");
    out.push('\n');
    Ok(out)
}

fn parse_guid(val: Option<&serde_json::Value>) -> Result<(u32, u32), FigError> {
    let obj = val.and_then(|v| v.as_object())
        .ok_or_else(|| FigError::Tree("missing or invalid guid".to_string()))?;
    let session = obj.get("sessionID").and_then(|v| v.as_u64())
        .ok_or_else(|| FigError::Tree("invalid sessionID in guid".to_string()))? as u32;
    let local = obj.get("localID").and_then(|v| v.as_u64())
        .ok_or_else(|| FigError::Tree("invalid localID in guid".to_string()))? as u32;
    Ok((session, local))
}

fn walk_tree(
    guid: (u32, u32),
    depth: usize,
    nodes: &HashMap<(u32, u32), &serde_json::Value>,
    children_map: &HashMap<(u32, u32), Vec<&serde_json::Value>>,
    lines: &mut Vec<String>,
) -> Result<(), FigError> {
    let node = nodes.get(&guid)
        .ok_or_else(|| FigError::Tree(format!("missing node {guid:?}")))?;

    let node_type = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let name = node.get("name").and_then(|v| v.as_str()).unwrap_or("");

    let indent = "  ".repeat(depth);
    lines.push(format!("{indent}{node_type} {}:{} {name}", guid.0, guid.1));

    if let Some(children) = children_map.get(&guid) {
        let mut sorted_children = children.clone();
        sorted_children.sort_by(|a, b| {
            let pos_a = a.get("parentIndex").and_then(|p| p.get("position")).and_then(|p| p.as_str()).unwrap_or("");
            let pos_b = b.get("parentIndex").and_then(|p| p.get("position")).and_then(|p| p.as_str()).unwrap_or("");
            pos_a.cmp(pos_b)
        });
        for child in sorted_children {
            let child_guid = parse_guid(child.get("guid"))?;
            walk_tree(child_guid, depth + 1, nodes, children_map, lines)?;
        }
    }

    Ok(())
}
