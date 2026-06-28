use super::*;

#[test]
fn public_quote_text_removes_wiki_and_html_markup() {
    let text = "{{block center/s|width=80%}}<center>'''第六支，樂中悲：'''</center> 襁褓中，父母嘆雙亡。{{~~|【甲側：意真辭切。】}}<br>終久是雲散高唐。<ref>這是校注，不應进入公开短引。</ref>";
    let quote = public_quote_text(text);

    assert!(!quote.contains("{{"));
    assert!(!quote.contains("<center>"));
    assert!(!quote.contains("block center"));
    assert!(!quote.contains("校注"));
    assert!(quote.contains("第六支，樂中悲"));
    assert!(quote.contains("甲側：意真辭切"));
}
