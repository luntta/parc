//! parc's tokenizer for the meaning model (`embed::Tokenizer`) against the
//! `tokenizers` crate it stands in for: the same token IDs for every text,
//! with every installed model of the catalog. Skipped (with a note) when no
//! model is installed; `PARC_MODELS_DIR` points elsewhere.
//! `PARC_TOKENIZER_CORPUS=<folder>` adds every `.md` file under a folder.

use std::path::{Path, PathBuf};

use parc_core::embed;

fn corpus() -> Vec<String> {
    let mut texts: Vec<String> = [
        "Hello, world!",
        "  leading and trailing  ",
        "tabs\tand\nnewlines\r\nand  double  spaces",
        "Kävin kirjastossa; Åland, Öljy, ÄÖÅ äöå",
        "naïve café, ﬁne ligature, Ｆｕｌｌｗｉｄｔｈ １２３, ½ ¼",
        "😀 emoji, 👩‍👩‍👧 family, 🇫🇮 flag, ❤️ heart",
        "日本語のテキスト、中文文本，한국어 텍스트",
        "العربية والفارسی, русский текст, Ελληνικά, עברית",
        "combining e\u{301} and a\u{308}, zero\u{200b}width, nbsp\u{a0}here, ideographic\u{3000}space",
        "\u{2581}already marked, a\u{2581}b, trailing\u{2581}",
        "control \u{7} bell, replacement \u{fffd}, soft\u{ad}hyphen",
        "ẞ ß İstanbul ı ǅ ǈ",
        "∑∫√ ≠ ≤ → ← ⇒ ∞ °C",
        "https://example.com/path?q=1&r=2#frag, user@example.com",
        "code `let x = vec![1, 2, 3];` and **bold** _em_ #tag [[01JQ7V4Y|label]]",
        "okay so um the tomato starts are doing the the thing again",
        "Polvessa taas vihlontaa. Pitkän kävelyn jälkeen polvessa vihloo, varsinkin alamäessä.",
        "",
        " ",
        "a",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    texts.push("x".repeat(300));
    texts.push("Saippuakivikauppias ".repeat(40));
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/recall");
    for entry in std::fs::read_dir(fixtures).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "json") {
            let file: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            for f in file["fragments"].as_array().unwrap() {
                texts.push(format!(
                    "{}\n{}",
                    f["title"].as_str().unwrap(),
                    f["body"].as_str().unwrap()
                ));
            }
        }
    }
    if let Some(dir) = std::env::var_os("PARC_TOKENIZER_CORPUS") {
        markdown_under(&PathBuf::from(dir), &mut texts);
    }
    texts
}

fn markdown_under(dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            markdown_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.extend(text.lines().map(str::to_string));
                out.push(text);
            }
        }
    }
}

#[test]
fn the_tokenizer_gives_the_tokens_hugging_faces_does() {
    let texts = corpus();
    let mut checked = 0;
    for spec in embed::CATALOG {
        let Some(dir) = embed::model_dir(spec).filter(|d| embed::is_installed(spec, d)) else {
            continue;
        };
        let file = dir.join("tokenizer.json");
        let mut theirs = tokenizers::Tokenizer::from_file(&file).unwrap();
        // parc reads long texts in windows of its own.
        theirs.with_truncation(None).unwrap();
        theirs.with_padding(None);
        let ours = embed::Tokenizer::from_file(&file).unwrap();
        for text in &texts {
            let want = theirs
                .encode(text.as_str(), false)
                .unwrap()
                .get_ids()
                .to_vec();
            assert_eq!(ours.encode(text), want, "{}: {text:?}", spec.id);
        }
        let marks = theirs.encode("", true).unwrap().get_ids().to_vec();
        assert_eq!(
            [ours.start.clone(), ours.end.clone()].concat(),
            marks,
            "{}",
            spec.id
        );
        checked += 1;
    }
    if checked == 0 {
        eprintln!(
            "skipped: no meaning model installed in {:?}",
            embed::models_dir()
        );
    }
}
