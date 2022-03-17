use std::collections::HashMap;

use fst::Set;
use once_cell::sync::Lazy;

use crate::detection::is_latin;
use crate::normalizer::{
    ControlCharacterRemover, DeunicodeNormalizer, LowercaseNormalizer, Normalizer,
};
use crate::processors::{
    ChineseTranslationPreProcessor, IdentityPreProcessor, PreProcessor, ProcessedText,
};
use crate::token_classifier::TokenClassifier;
use crate::tokenizer::{Jieba, LegacyMeilisearch, Lindera, TokenStream, Tokenizer};
use crate::Token;

static DEFAULT_PIPELINE: Lazy<Pipeline> = Lazy::new(Pipeline::default);

pub struct Pipeline {
    pre_processor: Box<dyn PreProcessor + 'static>,
    tokenizer: Box<dyn Tokenizer + 'static>,
    normalizer: Box<dyn Normalizer + 'static>,
}

impl Default for Pipeline {
    fn default() -> Self {
        // Hotfix: make a common default normalizer for every pipeline
        let deunicoder =
            DeunicodeNormalizer::new(&|text: &str| !text.chars().next().map_or(true, is_latin));
        let normalizer: Vec<Box<dyn Normalizer>> = vec![
            Box::new(deunicoder),
            Box::new(LowercaseNormalizer),
            Box::new(ControlCharacterRemover),
        ];

        Self {
            pre_processor: Box::new(IdentityPreProcessor),
            tokenizer: Box::new(LegacyMeilisearch),
            normalizer: Box::new(normalizer),
        }
    }
}

impl Pipeline {
    pub fn set_pre_processor(mut self, pre_processor: impl PreProcessor + 'static) -> Self {
        self.pre_processor = Box::new(pre_processor);
        self
    }

    pub fn set_tokenizer(mut self, tokenizer: impl Tokenizer + 'static) -> Self {
        self.tokenizer = Box::new(tokenizer);
        self
    }

    pub fn set_normalizer(mut self, normalizer: impl Normalizer + 'static) -> Self {
        self.normalizer = Box::new(normalizer);
        self
    }
}

macro_rules! make_language {
    ($($language:tt), +) => {
        #[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
        pub enum Language {
            $($language),+,
            Other,
        }
        impl From<whatlang::Lang> for Language {
            fn from(other: whatlang::Lang) -> Language {
                match other {
                    $(whatlang::Lang::$language => Language::$language), +
                }
            }
        }
    };
}

make_language! {
    Epo,
    Eng,
    Rus,
    Cmn,
    Spa,
    Por,
    Ita,
    Ben,
    Fra,
    Deu,
    Ukr,
    Kat,
    Ara,
    Hin,
    Jpn,
    Heb,
    Yid,
    Pol,
    Amh,
    Jav,
    Kor,
    Nob,
    Dan,
    Swe,
    Fin,
    Tur,
    Nld,
    Hun,
    Ces,
    Ell,
    Bul,
    Bel,
    Mar,
    Kan,
    Ron,
    Slv,
    Hrv,
    Srp,
    Mkd,
    Lit,
    Lav,
    Est,
    Tam,
    Vie,
    Urd,
    Tha,
    Guj,
    Uzb,
    Pan,
    Aze,
    Ind,
    Tel,
    Pes,
    Mal,
    Ori,
    Mya,
    Nep,
    Sin,
    Khm,
    Tuk,
    Aka,
    Zul,
    Sna,
    Afr,
    Lat,
    Slk,
    Cat,
    Tgl
}

macro_rules! make_script {
    ($($script:tt), +) => {
        #[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
        pub enum Script {
            $($script),+,
            Other,
        }

        impl From<whatlang::Script> for Script {
            fn from(other: whatlang::Script) -> Script {
                match other {
                    $(whatlang::Script::$script => Script::$script), +
                }
            }

        }
    };
}

make_script! {
    Arabic,
    Bengali,
    Cyrillic,
    Devanagari,
    Ethiopic,
    Georgian,
    Greek,
    Gujarati,
    Gurmukhi,
    Hangul,
    Hebrew,
    Hiragana,
    Kannada,
    Katakana,
    Khmer,
    Latin,
    Malayalam,
    Mandarin,
    Myanmar,
    Oriya,
    Sinhala,
    Tamil,
    Telugu,
    Thai
}

pub struct AnalyzerConfig<'a, A> {
    /// language specialized tokenizer, this can be switched during
    /// document tokenization if the document contains several languages
    pub pipeline_map: HashMap<(Script, Language), Pipeline>,
    pub stop_words: Option<&'a Set<A>>,
}

impl<'a, A> AnalyzerConfig<'a, A> {
    pub fn stop_words(&mut self, stop_words: &'a Set<A>) -> &mut Self {
        self.stop_words = Some(stop_words);
        self
    }
}

impl<A> AnalyzerConfig<'_, A> {
    pub fn new(pipeline_map: HashMap<(Script, Language), Pipeline>) -> Self {
        Self { pipeline_map, stop_words: None }
    }
}

impl<A> Default for AnalyzerConfig<'_, A> {
    fn default() -> Self {
        let mut pipeline_map: HashMap<(Script, Language), Pipeline> = HashMap::new();

        // Latin script specialized pipeline
        pipeline_map.insert(
            (Script::Latin, Language::Other),
            Pipeline::default().set_tokenizer(LegacyMeilisearch),
        );

        // Chinese script specialized pipeline
        pipeline_map.insert(
            (Script::Mandarin, Language::Cmn),
            Pipeline::default()
                .set_pre_processor(ChineseTranslationPreProcessor)
                .set_tokenizer(Jieba::default()),
        );

        // Japanese Kanji pipeline
        pipeline_map.insert(
            (Script::Mandarin, Language::Jpn),
            Pipeline::default().set_tokenizer(Lindera::default()),
        );

        // Japanese Hiragana pipeline
        pipeline_map.insert(
            (Script::Hiragana, Language::Jpn),
            Pipeline::default().set_tokenizer(Lindera::default()),
        );

        // Japanese Katakana pipeline
        pipeline_map.insert(
            (Script::Katakana, Language::Jpn),
            Pipeline::default().set_tokenizer(Lindera::default()),
        );

        AnalyzerConfig { pipeline_map, stop_words: None }
    }
}

pub struct Analyzer<'a, A> {
    config: AnalyzerConfig<'a, A>,
}

pub struct AnalyzedText<'a, 't, A> {
    /// Processed text
    processed: ProcessedText<'t>,
    /// Pipeline used to proccess the text
    pipeline: &'a Pipeline,
    /// Classifier used to give token a kind
    classifier: TokenClassifier<'a, A>,
}

impl<'a, 't, A> AnalyzedText<'a, 't, A>
where
    A: AsRef<[u8]>,
{
    /// Returns a `TokenStream` for the Analyzed text.
    pub fn tokens(&'t self) -> TokenStream<'t> {
        let stream = self
            .pipeline
            .tokenizer
            .tokenize(&self.processed)
            .map(move |t| self.pipeline.normalizer.normalize(t))
            .map(move |t| self.classifier.classify(t));
        TokenStream { inner: Box::new(stream) }
    }

    /// Attaches each token to its corresponding portion of the original text.
    pub fn reconstruct(&'t self) -> impl Iterator<Item = (&'t str, Token<'t>)> {
        self.tokens().map(move |t| (&self.processed.original[t.byte_start..t.byte_end], t))
    }
}

impl<'a, A> Analyzer<'a, A> {
    /// create a new tokenizer detecting script
    /// and chose the specialized internal tokenizer
    pub fn new(config: AnalyzerConfig<'a, A>) -> Self {
        Self { config }
    }

    /// Builds an `AnalyzedText` instance with the correct analyzer pipeline, and pre-processes the
    /// text.
    ///
    /// If an analysis pipeline exists for the inferred `(Script, Language)`, the analyzer will look
    /// for a user specified default `(Script::Other, Language::Other)`. If the user default is not
    /// specified, it will fallback to `(IdentityPreProcessor, UnicodeSegmenter, IdentityNormalizer)`.
    ///
    /// ```rust
    /// use meilisearch_tokenizer::{Analyzer, AnalyzerConfig};
    /// use fst::Set;
    /// // defaults to unicode segmenter with identity preprocessor and normalizer.
    /// let stop_words = Set::from_iter([""].iter()).unwrap();
    /// let mut config = AnalyzerConfig::default();
    /// config.stop_words(&stop_words);
    /// let analyzer = Analyzer::new(config);
    /// let analyzed = analyzer.analyze("The quick (\"brown\") fox can't jump 32.3 feet, right? Brr, it's 29.3°F!");
    /// let mut tokens = analyzed.tokens();
    /// assert!("the" == tokens.next().unwrap().text());
    /// ```
    pub fn analyze<'t>(&'a self, text: &'t str) -> AnalyzedText<'a, 't, A> {
        let pipeline = self.pipeline_from(text);
        let processed = pipeline.pre_processor.process(text);
        let classifier = TokenClassifier::new(self.config.stop_words);
        AnalyzedText { processed, pipeline, classifier }
    }

    /// Try to Detect Language and Script and return the corresponding pipeline,
    /// if no Language is detected or no pipeline corresponds to the Language
    /// the function try to get a pipeline corresponding to the script;
    /// if no Script is detected or no pipeline corresponds to the Script,
    /// the function try to get the default pipeline in the map;
    /// if no default pipeline exist in the map return the librairy DEFAULT_PIPELINE.
    fn pipeline_from<'t>(&'a self, text: &'t str) -> &'a Pipeline {
        let detected_script = self.detect_script(text);
        let filtered_pipelines: Vec<_> = self
            .config
            .pipeline_map
            .iter()
            .filter(|((script, _), _)| *script == detected_script)
            .collect();
        match filtered_pipelines[..] {
            // no specialized pipeline found for this script,
            // choose the default one.
            [] => self
                .config
                .pipeline_map
                .get(&(Script::Other, Language::Other))
                .unwrap_or_else(|| &*DEFAULT_PIPELINE),
            // Only one specialized pipeline found,
            // we don't need to detect the Language.
            [((_, _), pipeline)] => pipeline,
            // several pipelines found,
            // we have to detect the language to get the good one.
            _ => {
                let detected_language = self.detect_lang(text);
                self.config
                    .pipeline_map
                    .get(&(detected_script, detected_language))
                    .or_else(|| self.config.pipeline_map.get(&(detected_script, Language::Other)))
                    .or_else(|| self.config.pipeline_map.get(&(Script::Other, Language::Other)))
                    .unwrap_or_else(|| &*DEFAULT_PIPELINE)
            }
        }
    }

    /// detect script with whatlang,
    /// if no script is detected, return Script::Other
    fn detect_script(&self, text: &str) -> Script {
        whatlang::detect_script(text).map(Script::from).unwrap_or(Script::Other)
    }

    /// detect lang with whatlang
    /// if no language is detected, return Language::Other
    fn detect_lang(&self, text: &str) -> Language {
        whatlang::detect_lang(text).map(Language::from).unwrap_or(Language::Other)
    }
}

#[cfg(test)]
mod test {
    use std::borrow::Cow;

    use super::*;
    use crate::normalizer::LowercaseNormalizer;
    use crate::TokenKind;

    #[test]
    fn test_simple_latin() {
        let analyzer = Analyzer::new(AnalyzerConfig::<Vec<u8>>::default());

        let orig = "The quick (\"brown\") fox can't jump 32.3 feet, right? Brr, it's 29.3°F!";
        let analyzed = analyzer.analyze(orig);
        let mut analyzed = analyzed.tokens();
        assert_eq!("the", analyzed.next().unwrap().text());
        assert_eq!(" ", analyzed.next().unwrap().text());
        assert_eq!("quick", analyzed.next().unwrap().text());
        assert_eq!(" (\"", analyzed.next().unwrap().text());
        assert_eq!("brown", analyzed.next().unwrap().text());
    }

    #[test]
    fn test_simple_chinese() {
        let analyzer = Analyzer::new(AnalyzerConfig::<Vec<u8>>::default());

        let orig = "人人生而自由﹐在尊严和权利上一律平等。他们赋有理性和良心﹐并应以兄弟关系的精神互相对待。";
        let analyzed = analyzer.analyze(orig);
        let analyzed: Vec<_> = analyzed.tokens().map(|token| token.word).collect();
        assert_eq!(
            analyzed,
            [
                "人人",
                "生而自由",
                "﹐",
                "在",
                "尊严",
                "和",
                "权利",
                "上",
                "一律平等",
                "。",
                "他们",
                "赋有",
                "理性",
                "和",
                "良心",
                "﹐",
                "并",
                "应以",
                "兄弟",
                "关系",
                "的",
                "精神",
                "互相",
                "对待",
                "。"
            ]
        );
    }

    #[test]
    fn test_traditional_chinese() {
        let analyzer = Analyzer::new(AnalyzerConfig::<Vec<u8>>::default());

        let traditional = "人人生而自由﹐在尊嚴和權利上一律平等。他們賦有理性和良心﹐並應以兄弟關係的精神互相對待。";
        let _simplified = "人人生而自由﹐在尊严和权利上一律平等。他们赋有理性和良心﹐并应以兄弟关系的精神互相对待。";

        let analyzed = analyzer.analyze(traditional);
        let analyzed: Vec<_> = analyzed.tokens().map(|token| token.word).collect();

        assert_eq!(
            analyzed,
            [
                "人人",
                "生而自由",
                "﹐",
                "在",
                "尊严",
                "和",
                "权利",
                "上",
                "一律平等",
                "。",
                "他们",
                "赋有",
                "理性",
                "和",
                "良心",
                "﹐",
                "并",
                "应以",
                "兄弟",
                "关系",
                "的",
                "精神",
                "互相",
                "对待",
                "。"
            ]
        );
    }

    #[test]
    fn test_mixed_languages() {
        let analyzer = Analyzer::new(AnalyzerConfig::<Vec<u8>>::default());

        let traditional = "ABB SáféRing CCCV Базовый\u{9}с реле SEG\u{00a0}WIC1, ТТ–W2+доп.катушка отключ 220 VAC+контакт сраб.реле 1НО+вывод слева+испытательные втулки. 生而自由";

        let analyzed = analyzer.analyze(traditional);
        let analyzed: Vec<_> = analyzed.tokens().map(|token| token.word).collect();

        assert_eq!(
            analyzed,
            [
                "abb",
                " ",
                "safering",
                " ",
                "cccv",
                " ",
                "базовый",
                "\u{9}",
                "с",
                " ",
                "реле",
                " ",
                "seg wic1",
                ", ",
                "тт",
                "–",
                "w2",
                "+",
                "доп",
                ".",
                "катушка",
                " ",
                "отключ",
                " ",
                "220",
                " ",
                "vac",
                "+",
                "контакт",
                " ",
                "сраб",
                ".",
                "реле",
                " ",
                "1но",
                "+",
                "вывод",
                " ",
                "слева",
                "+",
                "испытательные",
                " ",
                "втулки",
                ". ",
                "生",
                "而",
                "自",
                "由"
            ]
        );
    }

    #[test]
    fn test_simple_latin_with_lowercase_normalizer() {
        let mut pipeline_map: HashMap<(Script, Language), Pipeline> = HashMap::new();
        pipeline_map.insert(
            (Script::Latin, Language::Other),
            Pipeline::default().set_normalizer(LowercaseNormalizer),
        );

        let analyzer = Analyzer::new(AnalyzerConfig::<Vec<u8>>::new(pipeline_map));
        let orig = "The quick (\"brown\") fox can't jump 32.3 feet, right? Brr, it's 29.3°F!";
        let analyzed = analyzer.analyze(orig);
        assert_eq!("the", analyzed.tokens().next().unwrap().text());
    }

    #[test]
    fn test_reconstruct_latin() {
        let analyzer = Analyzer::new(AnalyzerConfig::<Vec<u8>>::default());
        let orig = "The quick (\"brown\") fox can't jump 32.3 feet, right? Brr, it's 29.3°F!";
        let tokens = analyzer.analyze(orig);
        assert_eq!(orig, tokens.reconstruct().map(|(t, _)| t).collect::<String>());
    }

    #[test]
    fn test_reconstruct_chinese() {
        let analyzer = Analyzer::new(AnalyzerConfig::<Vec<u8>>::default());
        let orig = "人人生而自由﹐在尊严和权利上一律平等。他们赋有理性和良心﹐并应以兄弟关系的精神互相对待。";
        let tokens = analyzer.analyze(orig);
        assert_eq!(orig, tokens.reconstruct().map(|(t, _)| t).collect::<String>());
    }

    #[test]
    fn test_reconstruct_korean() {
        let analyzer = Analyzer::new(AnalyzerConfig::<Vec<u8>>::default());
        let orig = "안녕하세요. 한의계에 새로운 흐름을 만들어갑니다.";
        let tokens = analyzer.analyze(orig);
        assert_eq!(orig, tokens.reconstruct().map(|(t, _)| t).collect::<String>());
    }

    #[test]
    fn test_reconstruct_traditional_chinese() {
        let analyzer = Analyzer::new(AnalyzerConfig::<Vec<u8>>::default());
        let traditional = "人人生而自由﹐在尊嚴和權利上一律平等。他們賦有理性和良心﹐並應以兄弟關係的精神互相對待。";
        let tokens = analyzer.analyze(traditional);
        assert_eq!(traditional, tokens.reconstruct().map(|(t, _)| t).collect::<String>());
    }

    #[test]
    fn test_meilisearch_1714() {
        let analyzer = Analyzer::new(AnalyzerConfig::<Vec<u8>>::default());

        let text = "小化妆包";
        let analyzed = analyzer.analyze(text);
        let analyzed: Vec<_> = analyzed.tokens().map(|token| token.word).collect();
        assert_eq!(analyzed, ["小", "化妆包"]);

        let text = "Ipad 包";
        let analyzed = analyzer.analyze(text);
        let analyzed: Vec<_> = analyzed.tokens().map(|token| token.word).collect();
        assert_eq!(analyzed, ["ipad", " ", "包"]);

        let text = "化妆";
        let analyzed = analyzer.analyze(text);
        let analyzed: Vec<_> = analyzed.tokens().map(|token| token.word).collect();
        assert_eq!(analyzed, ["化妆"]);

        let text = "小化妆";
        let analyzed = analyzer.analyze(text);
        let analyzed: Vec<_> = analyzed.tokens().map(|token| token.word).collect();
        assert_eq!(analyzed, ["小", "化妆"]);

        let text = "化妆包";
        let analyzed = analyzer.analyze(text);
        let analyzed: Vec<_> = analyzed.tokens().map(|token| token.word).collect();
        assert_eq!(analyzed, ["化妆包"]);

        let text = "小化妆包";
        let analyzed = analyzer.analyze(text);
        let analyzed: Vec<_> = analyzed.tokens().map(|token| token.word).collect();
        assert_eq!(analyzed, ["小", "化妆包"]);
    }

    #[test]
    fn test_num_chars_from_bytes() {
        let analyzer = Analyzer::new(AnalyzerConfig::<Vec<u8>>::default());

        let text = "Go💼od";
        let analyzed = analyzer.analyze(text);
        let mut analyzed = analyzed.tokens();
        let token = analyzed.next().unwrap();

        let num_chars = token.num_chars_from_bytes(11);
        assert_eq!(num_chars, 3);

        let num_chars = token.num_chars_from_bytes(10);
        assert_eq!(num_chars, 3);

        let num_chars = token.num_chars_from_bytes(9);
        assert_eq!(num_chars, 3);

        let num_chars = token.num_chars_from_bytes(2);
        assert_eq!(num_chars, 2);

        let num_chars = token.num_chars_from_bytes(1);
        assert_eq!(num_chars, 1);

        let num_chars = token.num_chars_from_bytes(13);
        assert_eq!(num_chars, 5);
    }

    #[test]
    fn test_num_chars_from_bytes_uninitialized() {
        let token = Token {
            kind: TokenKind::Word,
            word: Cow::Borrowed("word"),
            byte_start: 0,
            char_index: 0,
            byte_end: "word".len(),
            char_map: None,
        };

        let num_chars = token.num_chars_from_bytes(0);
        assert_eq!(num_chars, 0);

        let num_chars = token.num_chars_from_bytes(1);
        assert_eq!(num_chars, 1);

        let num_chars = token.num_chars_from_bytes(2);
        assert_eq!(num_chars, 2);

        let num_chars = token.num_chars_from_bytes(3);
        assert_eq!(num_chars, 3);

        let num_chars = token.num_chars_from_bytes(4);
        assert_eq!(num_chars, 4);

        let token = Token {
            kind: TokenKind::Word,
            word: Cow::Borrowed("Go💼od"),
            byte_start: 0,
            char_index: 0,
            byte_end: "Go💼od".len(),
            char_map: None,
        };

        let num_chars = token.num_chars_from_bytes(1);
        assert_eq!(num_chars, 1);

        let num_chars = token.num_chars_from_bytes(2);
        assert_eq!(num_chars, 2);

        // consider the char even if only a part of it is available.
        let num_chars = token.num_chars_from_bytes(3);
        assert_eq!(num_chars, 3);

        let num_chars = token.num_chars_from_bytes(6);
        assert_eq!(num_chars, 3);

        let num_chars = token.num_chars_from_bytes(7);
        assert_eq!(num_chars, 4);

        let num_chars = token.num_chars_from_bytes(8);
        assert_eq!(num_chars, 5);
    }
    #[test]
    fn test_simple_japanese() {
        let analyzer = Analyzer::new(AnalyzerConfig::<Vec<u8>>::default());

        let orig = "関西国際空港限定トートバッグ";
        let analyzed = analyzer.analyze(orig);
        let analyzed: Vec<_> = analyzed.tokens().map(|token| token.word).collect();
        assert_eq!(analyzed, ["関西", "国際", "空港", "限定", "トートバッグ"]);
    }
}
