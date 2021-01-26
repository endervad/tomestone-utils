use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    panic::{catch_unwind, RefUnwindSafe, UnwindSafe},
    path::PathBuf,
};

use nom::{error::Error, IResult};
use sha1::{Digest, Sha1};
use tomestone_sqpack::{
    list_repositories,
    parser::{
        drive_streaming_parser, index_entry_1, index_entry_2, index_segment_headers,
        sqpack_header_outer, GrowableBufReader,
    },
    IndexEntry, IndexEntry1, IndexEntry2, IndexSegmentHeader,
};

fn forall_sqpack(f: impl Fn(PathBuf, GrowableBufReader<File>) + UnwindSafe + RefUnwindSafe) {
    dotenv::dotenv().unwrap();
    // Don't test anything if the game directory isn't provided
    if let Ok(root) = std::env::var("FFXIV_INSTALL_DIR") {
        let repositories = list_repositories(&root).unwrap();
        let sqpack_dir = PathBuf::from(root).join("game").join("sqpack");
        for repository in repositories {
            let path = sqpack_dir.join(repository);
            for res in std::fs::read_dir(path).unwrap() {
                let file_entry = res.unwrap();
                let path = file_entry.path();
                let file = File::open(&path).unwrap();
                let res = catch_unwind(|| f(path, GrowableBufReader::new(file)));
                if let Err(panic) = res {
                    eprintln!("Error while processing {:?}", file_entry.path());
                    panic!(panic);
                }
            }
        }
    }
}

#[test]
fn parse_game_data() {
    forall_sqpack(|path, mut bufreader| match path.extension() {
        Some(ext)
            if ext.to_string_lossy().starts_with("dat") || ext == "index" || ext == "index2" =>
        {
            let parsed = drive_streaming_parser::<_, _, _, Error<&[u8]>>(
                &mut bufreader,
                sqpack_header_outer,
            )
            .unwrap()
            .unwrap();
            println!("{:?}", parsed);
            if ext == "index" || ext == "index2" {
                let size = parsed.1;
                bufreader.seek(SeekFrom::Start(size.into())).unwrap();
                let parsed = drive_streaming_parser::<_, _, _, Error<&[u8]>>(
                    &mut bufreader,
                    index_segment_headers,
                )
                .unwrap()
                .unwrap();
                println!("{:?}", parsed);
            }
        }
        _ => {}
    });
}

#[test]
fn check_index_hashes() {
    forall_sqpack(|path, mut bufreader| match path.extension() {
        Some(ext) if ext == "index" || ext == "index2" => {
            let parsed = drive_streaming_parser::<_, _, _, Error<&[u8]>>(
                &mut bufreader,
                sqpack_header_outer,
            )
            .unwrap()
            .unwrap();
            let size = parsed.1;
            bufreader.seek(SeekFrom::Start(size.into())).unwrap();
            let parsed = drive_streaming_parser::<_, _, _, Error<&[u8]>>(
                &mut bufreader,
                index_segment_headers,
            )
            .unwrap()
            .unwrap();
            for header in &parsed.1 {
                if header.size == 0 {
                    continue;
                }
                bufreader
                    .seek(SeekFrom::Start(header.offset.into()))
                    .unwrap();
                let mut buf = vec![0; header.size as usize];
                bufreader.read_exact(&mut buf).unwrap();
                let mut hash = Sha1::new();
                hash.update(&buf);
                assert_eq!(*hash.finalize(), header.hash);
            }
        }
        _ => {}
    });
}

#[test]
fn check_index_order() {
    fn inner<I: IndexEntry, P: Fn(&[u8]) -> IResult<&[u8], I>>(
        header: &IndexSegmentHeader,
        bufreader: &mut GrowableBufReader<File>,
        parser: P,
    ) {
        let mut last_hash: Option<I::Hash> = None;
        for _ in 0..(header.size / I::SIZE) {
            let index_entry = drive_streaming_parser::<_, _, _, Error<&[u8]>>(bufreader, &parser)
                .unwrap()
                .unwrap();
            let hash = index_entry.hash();
            if let Some(last_hash) = &last_hash {
                assert!(last_hash < &hash);
            }
            last_hash = Some(hash);
        }
    }

    forall_sqpack(|path, mut bufreader| match path.extension() {
        Some(ext) if ext == "index" || ext == "index2" => {
            let parsed = drive_streaming_parser::<_, _, _, Error<&[u8]>>(
                &mut bufreader,
                sqpack_header_outer,
            )
            .unwrap()
            .unwrap();
            let size = parsed.1;
            bufreader.seek(SeekFrom::Start(size.into())).unwrap();
            let parsed = drive_streaming_parser::<_, _, _, Error<&[u8]>>(
                &mut bufreader,
                index_segment_headers,
            )
            .unwrap()
            .unwrap();
            let header = &parsed.1[0];
            if header.size == 0 {
                return;
            }
            bufreader
                .seek(SeekFrom::Start(header.offset.into()))
                .unwrap();
            if ext == "index" {
                inner::<IndexEntry1, _>(&header, &mut bufreader, index_entry_1);
            } else {
                inner::<IndexEntry2, _>(&header, &mut bufreader, index_entry_2);
            }
        }
        _ => {}
    });
}
