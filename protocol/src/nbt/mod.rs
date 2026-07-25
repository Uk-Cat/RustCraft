// Copyright 2016 Matthew Collins
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::HashMap;
use std::io;
use std::io::Read;

use super::protocol;
use super::protocol::Serializable;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

#[derive(Debug, Clone)]
pub enum Tag {
    End,
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<Tag>),
    Compound(HashMap<String, Tag>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

#[derive(Debug, Clone)]
pub struct NamedTag(pub String, pub Tag);

impl Tag {
    pub fn new_compound() -> Tag {
        Tag::Compound(HashMap::new())
    }

    pub fn new_list() -> Tag {
        Tag::List(Vec::new())
    }

    /// Returns the tag with the given name from the compound.
    ///
    /// # Panics
    /// Panics when the tag isn't a compound.
    pub fn get(&self, name: &str) -> Option<&Tag> {
        match *self {
            Tag::Compound(ref val) => val.get(name),
            _ => panic!("not a compound tag"),
        }
    }

    /// Places the tag into the compound using the given name.
    ///
    /// # Panics
    /// Panics when the tag isn't a compound.
    pub fn put(&mut self, name: &str, tag: Tag) {
        match *self {
            Tag::Compound(ref mut val) => val.insert(name.to_owned(), tag),
            _ => panic!("not a compound tag"),
        };
    }

    pub fn is_compound(&self) -> bool {
        matches!(*self, Tag::Compound(_))
    }

    pub fn as_byte(&self) -> Option<i8> {
        match *self {
            Tag::Byte(val) => Some(val),
            _ => None,
        }
    }

    pub fn as_short(&self) -> Option<i16> {
        match *self {
            Tag::Short(val) => Some(val),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i32> {
        match *self {
            Tag::Int(val) => Some(val),
            _ => None,
        }
    }

    pub fn as_long(&self) -> Option<i64> {
        match *self {
            Tag::Long(val) => Some(val),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f32> {
        match *self {
            Tag::Float(val) => Some(val),
            _ => None,
        }
    }

    pub fn as_double(&self) -> Option<f64> {
        match *self {
            Tag::Double(val) => Some(val),
            _ => None,
        }
    }

    pub fn as_byte_array(&self) -> Option<&[u8]> {
        match *self {
            Tag::ByteArray(ref val) => Some(&val[..]),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match *self {
            Tag::String(ref val) => Some(&val[..]),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Tag]> {
        match *self {
            Tag::List(ref val) => Some(&val[..]),
            _ => None,
        }
    }

    pub fn as_compound(&self) -> Option<&HashMap<String, Tag>> {
        match *self {
            Tag::Compound(ref val) => Some(val),
            _ => None,
        }
    }

    pub fn as_int_array(&self) -> Option<&[i32]> {
        match *self {
            Tag::IntArray(ref val) => Some(&val[..]),
            _ => None,
        }
    }

    pub fn as_long_array(&self) -> Option<&[i64]> {
        match *self {
            Tag::LongArray(ref val) => Some(&val[..]),
            _ => None,
        }
    }

    fn internal_id(&self) -> u8 {
        match *self {
            Tag::End => 0,
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(_) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }

    fn read_type<R: io::Read>(id: u8, buf: &mut R) -> Result<Tag, protocol::Error> {
        read_type_depth(id, buf, 0)
    }
}

/// Maximum NBT nesting depth. Real Minecraft NBT (dimension codecs,
/// chunk NBT, etc.) rarely exceeds 10–15 levels. 512 is a generous cap
/// that allows any legitimate data while preventing stack overflow on
/// Windows (where the default thread stack is only 1 MB, vs 8 MB on
/// Linux). A malicious 30 KB NBT blob can encode ~7500 nested compounds,
/// which would use ~1.5 MB of stack and overflow a 1 MB Windows stack.
/// The depth cap converts this into a clean Err instead of a process
/// kill.
const MAX_NBT_DEPTH: u32 = 512;

fn read_type_depth<R: io::Read>(
    id: u8,
    buf: &mut R,
    depth: u32,
) -> Result<Tag, protocol::Error> {
    if depth > MAX_NBT_DEPTH {
        return Err(protocol::Error::Err(format!(
            "NBT: nesting depth exceeds {} (likely malformed data), aborting parse",
            MAX_NBT_DEPTH
        )));
    }
    match id {
        0 => {
            // TAG_End should only be encountered by the Compound
            // parser (which checks for it before calling read_type).
            // If we get here, the NBT stream is malformed — return
            // an error rather than panicking via `unreachable!()`.
            Err(protocol::Error::Err(
                "NBT: unexpected TAG_End in read_type".to_owned(),
            ))
        }
        1 => Ok(Tag::Byte(buf.read_i8()?)),
        2 => Ok(Tag::Short(buf.read_i16::<BigEndian>()?)),
        3 => Ok(Tag::Int(buf.read_i32::<BigEndian>()?)),
        4 => Ok(Tag::Long(buf.read_i64::<BigEndian>()?)),
        5 => Ok(Tag::Float(buf.read_f32::<BigEndian>()?)),
        6 => Ok(Tag::Double(buf.read_f64::<BigEndian>()?)),
        7 => Ok(Tag::ByteArray({
            let len: i32 = Serializable::read_from(buf)?;
            // Hard-cap the allocation to 16 MiB so a malformed or
            // malicious NBT can't OOM-kill the client. Real
            // ByteArray tags in MC are tiny (block-light arrays,
            // etc.) so this cap never bites in practice.
            let len = sanitize_array_len(len)?;
            let mut data = Vec::with_capacity(len);
            buf.take(len as u64).read_to_end(&mut data)?;
            data
        })),
        8 => Ok(Tag::String(read_string(buf)?)),
        9 => {
            let mut l = Vec::new();
            let ty = buf.read_u8()?;
            let len: i32 = Serializable::read_from(buf)?;
            let len = sanitize_array_len(len)? as i64;
            for _ in 0..len {
                l.push(read_type_depth(ty, buf, depth + 1)?);
            }
            Ok(Tag::List(l))
        }
        10 => {
            let mut c = Tag::new_compound();
            // Cap the number of compound entries to prevent a
            // malicious server from sending an infinitely-nested
            // or never-terminated compound that would loop forever
            // allocating memory. 65536 entries is far beyond
            // anything real MC NBT contains.
            const MAX_COMPOUND_ENTRIES: usize = 65536;
            let mut entries = 0usize;
            loop {
                let ty = buf.read_u8()?;
                if ty == 0 {
                    break;
                }
                entries += 1;
                if entries > MAX_COMPOUND_ENTRIES {
                    return Err(protocol::Error::Err(format!(
                        "NBT: compound tag has more than {} entries, aborting parse",
                        MAX_COMPOUND_ENTRIES
                    )));
                }
                let name: String = read_string(buf)?;
                c.put(&name[..], read_type_depth(ty, buf, depth + 1)?);
            }
            Ok(c)
        }
        11 => Ok(Tag::IntArray({
            let len: i32 = Serializable::read_from(buf)?;
            let len = sanitize_array_len(len)?;
            let mut data = Vec::with_capacity(len);
            for _ in 0..len {
                data.push(buf.read_i32::<BigEndian>()?);
            }
            data
        })),
        12 => Ok(Tag::LongArray({
            let len: i32 = Serializable::read_from(buf)?;
            let len = sanitize_array_len(len)?;
            let mut data = Vec::with_capacity(len);
            for _ in 0..len {
                data.push(buf.read_i64::<BigEndian>()?);
            }
            data
        })),
        _ => Err(protocol::Error::Err(format!("invalid tag id: {}", id))),
    }
}


impl Serializable for Tag {
    fn read_from<R: io::Read>(buf: &mut R) -> Result<Tag, protocol::Error> {
        Tag::read_type(10, buf)
    }

    fn write_to<W: io::Write>(&self, buf: &mut W) -> Result<(), protocol::Error> {
        match *self {
            Tag::End => {}
            Tag::Byte(val) => buf.write_i8(val)?,
            Tag::Short(val) => buf.write_i16::<BigEndian>(val)?,
            Tag::Int(val) => buf.write_i32::<BigEndian>(val)?,
            Tag::Long(val) => buf.write_i64::<BigEndian>(val)?,
            Tag::Float(val) => buf.write_f32::<BigEndian>(val)?,
            Tag::Double(val) => buf.write_f64::<BigEndian>(val)?,
            Tag::ByteArray(ref val) => {
                (val.len() as i32).write_to(buf)?;
                buf.write_all(val)?;
            }
            Tag::String(ref val) => write_string(buf, val)?,
            Tag::List(ref val) => {
                if val.is_empty() {
                    buf.write_u8(0)?;
                    buf.write_i32::<BigEndian>(0)?;
                } else {
                    buf.write_u8(val[0].internal_id())?;
                    buf.write_i32::<BigEndian>(val.len() as i32)?;
                    for e in val {
                        e.write_to(buf)?;
                    }
                }
            }
            Tag::Compound(ref val) => {
                for (k, v) in val {
                    v.internal_id().write_to(buf)?;
                    write_string(buf, k)?;
                    v.write_to(buf)?;
                }
                buf.write_u8(0)?;
            }
            Tag::IntArray(ref val) => {
                (val.len() as i32).write_to(buf)?;
                for v in val {
                    v.write_to(buf)?;
                }
            }
            Tag::LongArray(ref val) => {
                (val.len() as i32).write_to(buf)?;
                for v in val {
                    v.write_to(buf)?;
                }
            }
        }
        Ok(())
    }
}

pub fn write_string<W: io::Write>(buf: &mut W, s: &str) -> Result<(), protocol::Error> {
    let data = s.as_bytes();
    (data.len() as i16).write_to(buf)?;
    buf.write_all(data).map_err(|v| v.into())
}

pub fn read_string<R: io::Read>(buf: &mut R) -> Result<String, protocol::Error> {
    let len: i16 = buf.read_i16::<BigEndian>()?;
    // NBT strings are limited to 32767 bytes by the format. If `len` is
    // negative (sign bit set on i16), `len as u16` would be a huge value
    // and `take(len as u64)` would block forever waiting for data.
    // Reject negative lengths explicitly.
    if len < 0 {
        return Err(protocol::Error::Err(format!(
            "NBT: negative string length {}",
            len
        )));
    }
    let mut bytes = Vec::<u8>::new();
    buf.take(len as u64).read_to_end(&mut bytes)?;
    // Use lossy decoding so a malformed UTF-8 string in NBT (which
    // shouldn't happen in vanilla but has been seen on modded servers)
    // doesn't panic the whole client.
    let ret = String::from_utf8(bytes)
        .unwrap_or_else(|e| String::from_utf8_lossy(&e.into_bytes()).into_owned());
    Ok(ret)
}

/// Sanity-checks an NBT array length so a malformed or malicious NBT
/// payload can't trigger a huge allocation (and OOM-kill the client) or
/// block forever on a `take(len as u64)` read.
///
/// Returns the length as a `usize` if it's in `[0, MAX_ARRAY_LEN]`,
/// otherwise an error. The cap is 16 MiB worth of elements — far beyond
/// anything real Minecraft NBT contains (chunk section block-light
/// arrays are 2048 bytes, biome arrays are 256 ints, etc.).
fn sanitize_array_len(len: i32) -> Result<usize, protocol::Error> {
    const MAX_ARRAY_LEN: i32 = 16 * 1024 * 1024; // 16 MiB worth of elements
    if len < 0 {
        return Err(protocol::Error::Err(format!(
            "NBT: negative array length {}",
            len
        )));
    }
    if len > MAX_ARRAY_LEN {
        return Err(protocol::Error::Err(format!(
            "NBT: array length {} exceeds the {} element safety cap, refusing to allocate",
            len, MAX_ARRAY_LEN
        )));
    }
    Ok(len as usize)
}
