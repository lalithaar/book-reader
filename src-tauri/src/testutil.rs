//! Test-only fixtures: a tiny but structurally complete EPUB.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;

pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bookreader-test-{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn make_epub(path: &Path) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let stored = SimpleFileOptions::default();
    let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();

    zip.start_file("META-INF/container.xml", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0"?>
        <container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
          <rootfiles>
            <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
          </rootfiles>
        </container>"#,
    )
    .unwrap();

    zip.start_file("OEBPS/content.opf", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="utf-8"?>
        <package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
          <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
            <dc:title>Test Book</dc:title>
            <dc:creator>Ada Lovelace</dc:creator>
            <dc:language>en</dc:language>
            <meta name="cover" content="cover-img"/>
          </metadata>
          <manifest>
            <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
            <item id="c1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/>
            <item id="c2" href="Text/ch2.xhtml" media-type="application/xhtml+xml"/>
            <item id="pic" href="Images/pic.png" media-type="image/png"/>
            <item id="cover-img" href="Images/cover.png" media-type="image/png"/>
          </manifest>
          <spine>
            <itemref idref="c1"/>
            <itemref idref="c2"/>
          </spine>
        </package>"#,
    )
    .unwrap();

    zip.start_file("OEBPS/nav.xhtml", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="utf-8"?>
        <html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
          <body>
            <nav epub:type="toc">
              <ol>
                <li><a href="Text/ch1.xhtml">Chapter One</a></li>
                <li><a href="Text/ch2.xhtml">Chapter Two</a></li>
              </ol>
            </nav>
          </body>
        </html>"#,
    )
    .unwrap();

    zip.start_file("OEBPS/Text/ch1.xhtml", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="utf-8"?>
        <html xmlns="http://www.w3.org/1999/xhtml">
          <head><title>ignored</title><style>p{color:red}</style></head>
          <body>
            <h1>One</h1>
            <p onclick="evil()">Hello <em>world</em>.</p>
            <script>alert(1)</script>
            <p><img src="../Images/pic.png" alt="pic"/></p>
            <p><a href="ch2.xhtml">next</a></p>
            <p><a href="https://example.com">away</a></p>
          </body>
        </html>"#,
    )
    .unwrap();

    zip.start_file("OEBPS/Text/ch2.xhtml", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="utf-8"?>
        <html xmlns="http://www.w3.org/1999/xhtml">
          <body><h1>Two</h1><p>Second chapter.</p></body>
        </html>"#,
    )
    .unwrap();

    zip.start_file("OEBPS/Images/pic.png", deflated).unwrap();
    zip.write_all(&[0x89, b'P', b'N', b'G']).unwrap();

    zip.start_file("OEBPS/Images/cover.png", deflated).unwrap();
    zip.write_all(&[0x89, b'P', b'N', b'G']).unwrap();

    zip.finish().unwrap();
}
