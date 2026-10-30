use std::fs;
use std::path::PathBuf;

pub fn load_system_cjk() -> Result<Vec<u8>, String> {
    let mut errors = Vec::new();
    for path in candidates() {
        if !path.exists() {
            continue;
        }
        match fs::read(&path).map_err(|err| err.to_string()).and_then(ensure_single_font) {
            Ok(bytes) => return Ok(bytes),
            Err(err) => errors.push(format!("{}：{err}", path.display())),
        }
    }
    if errors.is_empty() {
        Err("没有找到中文字体".into())
    } else {
        Err(format!("中文字体无法读取：{}", errors.join("；")))
    }
}

pub fn ensure_single_font(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.len() < 12 {
        return Err("字体文件太小".into());
    }
    if &bytes[0..4] == b"ttcf" {
        extract_face(&bytes, 0)
    } else {
        Ok(bytes)
    }
}

fn candidates() -> Vec<PathBuf> {
    let dir = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts");
    ["Deng.ttf", "simhei.ttf", "msyh.ttc", "simsun.ttc", "simfang.ttf", "simkai.ttf"]
        .into_iter()
        .map(|name| dir.join(name))
        .collect()
}

fn extract_face(data: &[u8], index: u32) -> Result<Vec<u8>, String> {
    if data.len() < 12 || &data[0..4] != b"ttcf" {
        return Err("不是字体集合".into());
    }
    let count = be_u32(data, 8)?;
    if index >= count {
        return Err("字体索引超出范围".into());
    }
    let font_off = be_u32(data, 12 + index as usize * 4)? as usize;
    if font_off.checked_add(12).map(|end| end > data.len()).unwrap_or(true) {
        return Err("字体头损坏".into());
    }
    let num_tables = be_u16(data, font_off + 4)? as usize;
    if num_tables == 0 || num_tables > 128 {
        return Err("字体表数量异常".into());
    }
    let dir_end = font_off
        .checked_add(12)
        .and_then(|value| value.checked_add(num_tables * 16))
        .ok_or("字体表目录损坏")?;
    if dir_end > data.len() {
        return Err("字体表目录损坏".into());
    }

    struct Rec {
        tag: [u8; 4],
        checksum: u32,
        offset: usize,
        length: usize,
    }
    let mut records = Vec::with_capacity(num_tables);
    for index in 0..num_tables {
        let at = font_off + 12 + index * 16;
        let mut tag = [0u8; 4];
        tag.copy_from_slice(&data[at..at + 4]);
        let offset = be_u32(data, at + 8)? as usize;
        let length = be_u32(data, at + 12)? as usize;
        let end = offset.checked_add(length).ok_or("字体表损坏")?;
        if end > data.len() {
            return Err("字体表超出文件".into());
        }
        records.push(Rec {
            tag,
            checksum: be_u32(data, at + 4)?,
            offset,
            length,
        });
    }

    let mut cursor = 12 + num_tables * 16;
    let mut new_offsets = Vec::with_capacity(num_tables);
    for record in &records {
        cursor = (cursor + 3) & !3;
        new_offsets.push(cursor);
        cursor = cursor.checked_add(record.length).ok_or("字体文件过大")?;
        if cursor > 80 * 1024 * 1024 {
            return Err("字体文件过大".into());
        }
    }

    let mut out = vec![0u8; cursor];
    out[0..12].copy_from_slice(&data[font_off..font_off + 12]);
    for (index, record) in records.iter().enumerate() {
        let at = 12 + index * 16;
        out[at..at + 4].copy_from_slice(&record.tag);
        put_u32(&mut out, at + 4, record.checksum);
        put_u32(&mut out, at + 8, new_offsets[index] as u32);
        put_u32(&mut out, at + 12, record.length as u32);
        let start = new_offsets[index];
        out[start..start + record.length].copy_from_slice(&data[record.offset..record.offset + record.length]);
    }
    Ok(out)
}

fn be_u16(data: &[u8], at: usize) -> Result<u16, String> {
    let bytes = data.get(at..at + 2).ok_or("字体文件损坏")?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn be_u32(data: &[u8], at: usize) -> Result<u32, String> {
    let bytes = data.get(at..at + 4).ok_or("字体文件损坏")?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn put_u32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_font_bytes_pass_through() {
        let mut bytes = vec![0x00, 0x01, 0x00, 0x00];
        bytes.extend(std::iter::repeat(0).take(8));
        let out = ensure_single_font(bytes.clone()).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn extracts_one_face_from_ttc() {
        let face = sample_face();
        let collection = wrap_ttc(&face);
        let out = ensure_single_font(collection).unwrap();
        assert_eq!(&out[0..4], &[0x00, 0x01, 0x00, 0x00]);
        assert!(out.windows(4).any(|item| item == b"ABCD"));
    }

    #[test]
    fn rejects_a_truncated_font() {
        assert!(ensure_single_font(vec![0, 1, 0, 0]).is_err());
    }

    #[test]
    fn windows_cjk_font_parses() {
        let bytes = load_system_cjk().expect("应该能读到系统中文字体");
        let face = ttf_parser::Face::parse(&bytes, 0).expect("中文字体无法解析");
        assert!(face.number_of_glyphs() > 1000);
    }

    fn sample_face() -> Vec<u8> {
        let table = b"ABCD";
        let offset = 28;
        let mut face = vec![0u8; offset + table.len()];
        face[0] = 0x00;
        face[1] = 0x01;
        face[5] = 0x01;
        face[12..16].copy_from_slice(b"test");
        face[20..24].copy_from_slice(&(offset as u32).to_be_bytes());
        face[24..28].copy_from_slice(&(table.len() as u32).to_be_bytes());
        face[offset..].copy_from_slice(table);
        face
    }

    fn wrap_ttc(face: &[u8]) -> Vec<u8> {
        let font_off = 16u32;
        let mut data = vec![0u8; 16 + face.len()];
        data[0..4].copy_from_slice(b"ttcf");
        data[8..12].copy_from_slice(&1u32.to_be_bytes());
        data[12..16].copy_from_slice(&font_off.to_be_bytes());
        data[16..].copy_from_slice(face);
        let old = u32::from_be_bytes(data[36..40].try_into().unwrap());
        data[36..40].copy_from_slice(&(old + font_off).to_be_bytes());
        data
    }
}
