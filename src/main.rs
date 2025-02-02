extern crate base64;
extern crate byteorder;
extern crate crypto;
extern crate id3;
extern crate json;
extern crate metaflac;
extern crate tempfile;

use byteorder::ByteOrder;
use byteorder::NativeEndian;
use crypto::buffer::{BufferResult, ReadBuffer, WriteBuffer};
use crypto::{aes, blockmodes, buffer, symmetriccipher};
use json::JsonValue;
use std::io::prelude::*;
use std::io::SeekFrom;
use std::mem;
use tempfile::NamedTempFile;

const AES_CORE_KEY: &[u8; 16] = b"\x68\x7A\x48\x52\x41\x6D\x73\x6F\x35\x6B\x49\x6E\x62\x61\x78\x57";
const AES_MODIFY_KEY: &[u8; 16] =
    b"\x23\x31\x34\x6C\x6A\x6B\x5F\x21\x5C\x5D\x26\x30\x55\x3C\x27\x28";
const FILTER: [(&str, &str); 8] = [
    ("\\", "＼"),
    ("/", "／"),
    (":", "："),
    ("*", "＊"),
    ("\"", "＂"),
    ("<", "＜"),
    (">", "＞"),
    ("|", "｜"),
];

fn build_key_box(key: &[u8]) -> [u8; 256] {
    let key_len = key.len();
    let mut tmpbox: [u8; 256] = [0; 256];

    for i in 0..256 {
        tmpbox[i] = i as u8;
    }
    //    let mut swap: u8;
    let mut c: u64;
    let mut last_byte: u64 = 0;

    for i in 0..256 {
        //        swap = tmpbox[i];
        c = (tmpbox[i] as u64 + last_byte + key[(i % key_len) as usize] as u64) & 0xff;
        //        tmpbox[i] = tmpbox[c as usize];
        //        tmpbox[c as usize] = swap;
        tmpbox.swap(i, c as usize);
        last_byte = c;
    }
    tmpbox
}

fn decrypt(
    encrypted_data: &[u8],
    key: &[u8],
) -> Result<Vec<u8>, symmetriccipher::SymmetricCipherError> {
    let mut decryptor = aes::ecb_decryptor(aes::KeySize::KeySize128, key, blockmodes::PkcsPadding);

    let mut final_result = Vec::<u8>::new();
    let mut read_buffer = buffer::RefReadBuffer::new(encrypted_data);
    let mut buffer = [0; 4096];
    let mut write_buffer = buffer::RefWriteBuffer::new(&mut buffer);

    loop {
        let result = decryptor
            .decrypt(&mut read_buffer, &mut write_buffer, true)
            .expect("Crypto decrypt error:");
        final_result.extend(
            write_buffer
                .take_read_buffer()
                .take_remaining()
                .iter()
                .map(|&i| i),
        );
        match result {
            BufferResult::BufferUnderflow => break,
            BufferResult::BufferOverflow => {}
        }
    }

    Ok(final_result)
}

fn process_file(path: &std::path::Path) -> std::io::Result<()> {
    let mut f = std::fs::File::open(path).expect("cannot open source file:");
    let mut buf = [0u8; mem::size_of::<u32>()];
    f.read(&mut buf)?;
    if NativeEndian::read_u32(&buf) != 0x4e455443 {
        panic!("Not a netease music file.")
    }
    f.read(&mut buf)?;
    if NativeEndian::read_u32(&buf) != 0x4d414446 {
        panic!("Not a netease music file.")
    }
    f.seek(SeekFrom::Current(2))?;
    let key_len: u32;
    f.read(&mut buf)?;
    key_len = NativeEndian::read_u32(&buf);
    let mut key_data: Vec<u8> = Vec::with_capacity(key_len as usize);
    key_data.resize(key_len as usize, 0);
    f.read_exact(&mut key_data)?;
    key_data.iter_mut().for_each(|x| *x ^= 0x64);

    //    let cipher = Cipher::aes_128_ecb();
    let de_key_data = decrypt(&key_data, AES_CORE_KEY).expect("error decrypting key data:");
    //        decrypt(cipher, AES_CORE_KEY, None, &key_data).expect("error decrypting key data:");
    //    let de_key_len = de_key_data.len() as u32;
    let kbox = build_key_box(&de_key_data[17..]);
    f.read(&mut buf)?;
    let ulen = NativeEndian::read_u32(&buf);
    let music_info = if ulen > 0 {
        let mut modify_data: Vec<u8> = Vec::with_capacity(ulen as usize);
        modify_data.resize(ulen as usize, 0);
        f.read_exact(&mut modify_data)?;
        modify_data.iter_mut().for_each(|x| *x ^= 0x63);
        // let data_len: usize;
        // let mut data: Vec<u8> = Vec::with_capacity(ulen as usize);
        // data.resize(ulen as usize, 0);
        // let mut dedata: Vec<u8> = Vec::with_capacity(ulen as usize);
        // dedata.resize(ulen as usize, 0);

        let data = base64::decode(&modify_data[22..]).expect("error decoding modify_data:");
        //    let dedata = decrypt(cipher, AES_MODIFY_KEY, None, &data).expect("error decrypting data:");
        let dedata = decrypt(&data, AES_MODIFY_KEY).expect("error decrypting data:");

        Some(
            json::parse(std::str::from_utf8(&dedata[6..]).expect("music info is not valid utf-8:"))
                .expect("error parsing json:"),
        )
    } else {
        println!(
            "{} has no metadata.",
            path.file_name().unwrap().to_str().unwrap()
        );
        None
    };
    let s = path.file_name().unwrap().to_str().unwrap();

    let music_filename = FILTER.iter().fold(
        s.get(0..s.len() - 4).unwrap().to_owned(),
        |acc, (fr, to)| acc.replace(fr, to),
    );

    println!("{}", music_filename);

    // f.read(&mut buf)?;
    // ulen = NativeEndian::read_u32(&buf);
    f.seek(SeekFrom::Current(9))?;
    f.read(&mut buf)?;
    let img_len: u32 = NativeEndian::read_u32(&buf);
    let img_data = if img_len > 0 {
        let mut t = Vec::with_capacity(img_len as usize);
        t.resize(img_len as usize, 0);
        f.read_exact(&mut t)?;
        Some(t)
    } else {
        println!(
            "{} has no cover image.",
            path.file_name().unwrap().to_str().unwrap()
        );
        None
    };
    let mut buffer = [0u8; 0x8000];
    let mut tmp = NamedTempFile::new()?;
    // let mut format = "undefined";
    loop {
        let n = f.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        for (i, x) in buffer.iter_mut().enumerate() {
            let j = (i + 1) & 0xff;
            *x ^= kbox[(kbox[j] as usize + kbox[(kbox[j] as usize + j) & 0xff] as usize) & 0xff];
        }
        tmp.write(&buffer)?;
        // if n > 2 {
        //     if format == "undefined" {
        //         format = if buffer[0..2] == [0x49, 0x44, 0x33] {
        //             "mp3"
        //         } else {
        //             "flac"
        //         };
        //     }
        // }
    }

    let tag = metaflac::Tag::read_from_path(tmp.path());
    let is_flac = match tag {
        Ok(_) => true,
        Err(_) => false,
    };

    let format = if is_flac { "flac" } else { "mp3" };

    // let format = format;
    let filter_music_filename = music_filename + "." + format;
    // let mut fmusic = std::fs::File::create(std::path::Path::new(&filter_music_filename))?;
    std::fs::copy(
        tmp.into_temp_path(),
        std::path::Path::new(&filter_music_filename),
    )?;
    // drop(tmp);
    drop(f);
    if let Some(music_info) = music_info {
        let mimetype = if let Some(ref img_data) = img_data {
            if img_data[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
                "image/png"
            } else {
                "image/jpeg"
            }
        } else {
            ""
        };
        let music_name = music_info["musicName"].as_str().unwrap();
        let album = music_info["album"].as_str().unwrap();
        let artist = &music_info["artist"];
        let _bitrate = music_info["bitrate"].as_i64().unwrap();
        let _duration = music_info["duration"].as_i64().unwrap();
        let artists: Vec<&str> = if let JsonValue::Array(arr) = artist {
            arr.iter()
                .map(|x| x[0].as_str().unwrap_or_default())
                .collect()
        } else {
            vec![]
        };
        if format == "mp3" {
            let mut tag = id3::Tag::read_from_path(std::path::Path::new(&filter_music_filename))
                .unwrap_or(id3::Tag::new());
            tag.set_title(music_name);
            tag.set_album(album);
            tag.set_artist(artists.join("/"));
            if let Some(img_data) = img_data {
                let picture = id3::frame::Picture {
                    mime_type: mimetype.to_owned(),
                    picture_type: id3::frame::PictureType::CoverFront,
                    description: String::new(),
                    data: img_data,
                };
                tag.add_picture(picture);
            }
            tag.write_to_path(
                std::path::Path::new(&filter_music_filename),
                id3::Version::Id3v23,
            )
            .expect("error writing MP3 file:");
        } else if format == "flac" {
            // flac
            let mut tag =
                metaflac::Tag::read_from_path(std::path::Path::new(&filter_music_filename))
                    .expect("error reading flac file:");
            let c = tag.vorbis_comments_mut();
            c.set_title(vec![music_name]);
            c.set_album(vec![album]);
            c.set_artist(artists);
            if let Some(img_data) = img_data {
                tag.add_picture(mimetype, metaflac::block::PictureType::CoverFront, img_data);
            }
            tag.write_to_path(std::path::Path::new(&filter_music_filename))
                .expect("error writing flac file:");
        }
    }
    Ok(())
}
fn main() {
    let arg = clap::Arg::new("input_file").index(1).required(true);
    let cmd = clap::Command::new("ncmdump").bin_name("ncmdump").arg(arg);
    let matches = cmd.get_matches();
    process_file(std::path::Path::new(
        matches.value_of("input_file").unwrap(),
    ))
    .expect("process error at main:");
}
