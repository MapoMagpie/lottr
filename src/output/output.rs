use std::{
    fs,
    io::{Read, Write},
};

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{input::TransType, textures::Textures, translator::Translator, Configuration};

use super::{mtool::MToolOutput, text::TextOutput};

pub fn output(config: &Configuration, textures: &Textures) -> Result<()> {
    match config.trans_type {
        TransType::Text => {
            if config.output_regexen.len() < 2 {
                return Err(anyhow::anyhow!("Please specify at least 2 regexes for MTool output! \n The MTool output need 2 regexes, one for the replace, and one for the capture."));
            }
            let output = TextOutput::new(
                &config.output_regexen[0].regex,
                &config.output_regexen[1].regex,
            );
            output.output(Translator::ChatGPT, textures);
        }
        TransType::MTool => {
            if config.output_regexen.len() < 2 {
                return Err(anyhow::anyhow!("Please specify at least 2 regexes for MTool output! \n The MTool output need 2 regexes, one for the replace, and one for the capture."));
            }
            let mut output = MToolOutput::new(
                &config.output_regexen[0].regex,
                &config.output_regexen[1].regex,
            );
            let line_width = config
                .mtool_opt
                .as_ref()
                .map(|v| v.line_width.clone())
                .flatten();
            output.set_line_width(line_width);
            output.output(Translator::ChatGPT, textures);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputRegex {
    pub usage: RegexUsage,
    pub regex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegexUsage {
    #[serde(rename = "replace")]
    Replace(String),
    #[serde(rename = "capture")]
    Capture(usize),
}

pub trait Output {
    fn output(&self, translator: Translator, textures: &Textures);
}

#[allow(dead_code)]
pub struct SimpleTextOutput {
    regex_chain: Vec<(RegexUsage, Regex)>,
}

#[allow(dead_code)]
impl SimpleTextOutput {
    pub fn new(clear_rules: Vec<OutputRegex>) -> Self {
        let regex_chain = clear_rules
            .into_iter()
            .map(|r| {
                let regex = Regex::new(&r.regex).unwrap();
                (r.usage, regex)
            })
            .collect::<Vec<_>>();
        Self { regex_chain }
    }

    pub fn clear(&self, content: &str) -> String {
        let mut content = content.to_string();
        for (usage, regex) in &self.regex_chain {
            match usage {
                RegexUsage::Replace(replace) => {
                    content = regex.replace_all(&content, replace).to_string();
                }
                RegexUsage::Capture(index) => {
                    let captures = regex.captures(&content);
                    if let Some(capture) = captures {
                        content = capture.get(*index).unwrap().as_str().to_string();
                    }
                }
            }
        }
        content
    }
}

impl Output for SimpleTextOutput {
    fn output(&self, translator: Translator, textures: &Textures) {
        let mut output_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(format!("{}.translated_{:?}.txt", textures.name, translator))
            .expect("Failed to open file");
        let mut i = 0;
        while i < textures.lines.len() {
            let line = &textures.lines[i];
            if let Some(translated) = line.translated.iter().find(|t| t.translator == translator) {
                let content = translated.content.as_str();
                let content = self.clear(content);
                output_file
                    .write(content.as_bytes())
                    .expect("Failed to write to file");
                if i != translated.batch_range.0 {
                    eprintln!(
                        "batch_range.0: {}, expected: {}",
                        translated.batch_range.0, i
                    );
                }
                i = translated.batch_range.1 + 1; // todo window
            } else {
                i += 1;
            }
        }
    }
}

pub trait RewriteOutput {
    fn extract_lines(&self, content: &str) -> Vec<String>;
    fn format_line(&self, raw: &str, content: &str) -> String;
}

impl<T> Output for T
where
    T: RewriteOutput,
{
    fn output(&self, translator: Translator, textures: &Textures) {
        let original_file = std::fs::OpenOptions::new()
            .read(true)
            .open(&textures.name)
            .expect(format!("Failed to open file {}", &textures.name).as_str());
        let ext = std::path::Path::new(&textures.name)
            .extension()
            .unwrap()
            .to_str()
            .unwrap();
        let rewritten_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(format!(
                "{}.translated_{:?}.{}",
                textures.name, translator, ext
            ))
            .expect(format!("Failed to open file {}", &textures.name).as_str());
        let mut reader = std::io::BufReader::new(original_file);
        let mut buf: [u8; 8192] = [0; 8192];
        let mut last_read_at = 0;
        let mut pre_read_at = 0;

        let mut writer = std::io::BufWriter::new(rewritten_file);
        let mut i = 0;

        let mut dignostic_failed_range = vec![];
        while i < textures.lines.len() {
            let line = &textures.lines[i];
            if let Some(translated) = line.translated.iter().find(|t| t.translator == translator) {
                if line.seek > pre_read_at {
                    reader
                        .seek_relative((pre_read_at - last_read_at) as i64)
                        .unwrap();
                    last_read_at = pre_read_at;
                    let mut size = line.seek - pre_read_at;
                    while size > 0 {
                        let buf_slice = if size > buf.len() {
                            &mut buf
                        } else {
                            &mut buf[..size]
                        };
                        let read_size = reader.read(buf_slice).unwrap();
                        last_read_at += read_size;
                        size -= read_size;
                        writer.write(&buf_slice[..read_size]).unwrap();
                    }
                    pre_read_at = line.seek;
                }
                let content = translated.content.as_str();
                let tran_lines = self.extract_lines(content);
                if tran_lines.len() != translated.batch_range.1 - translated.batch_range.0 + 1 {
                    dignostic_failed_range
                        .push((translated.batch_range.0, translated.batch_range.1));
                    i = translated.batch_range.1 + 1;
                    eprintln!(
                        "[Dignostic] batch range: {}-{}, expected size: {}, but extracted lines size: {}",
                        translated.batch_range.0,
                        translated.batch_range.1,
                        translated.batch_range.1 - translated.batch_range.0 + 1,
                        tran_lines.len()
                    );
                    let mut tran_lines_iter = tran_lines.iter();
                    let mut raw_lines_iter =
                        textures.lines[translated.batch_range.0..=translated.batch_range.1].iter();
                    loop {
                        let tran_line = tran_lines_iter.next();
                        let raw_line = raw_lines_iter.next();
                        if tran_line.is_none() || raw_line.is_none() {
                            break;
                        }
                        eprintln!(
                            "[Dignostic] raw: {}\n[Dignostic] tran: {}",
                            raw_line.unwrap().content,
                            tran_line.unwrap(),
                        );
                    }
                    continue;
                }
                let mut last_line_index_in_batch = 0;
                for (j, line) in tran_lines.iter().enumerate() {
                    let fmt = self.format_line(&textures.lines[i + j].content, line);
                    writer.write(fmt.as_bytes()).unwrap();
                    last_line_index_in_batch = i + j;
                }
                pre_read_at = textures.lines[last_line_index_in_batch].seek
                    + textures.lines[last_line_index_in_batch].size;

                // skip the batch
                i = translated.batch_range.1 + 1;
            } else {
                i += 1;
            }
        }
        reader
            .seek_relative((pre_read_at - last_read_at) as i64)
            .unwrap();
        // println!(
        //     "pre read at: {} last read at: {}",
        //     pre_read_at, last_read_at
        // );
        loop {
            let size = reader.read(&mut buf).unwrap();
            if size == 0 {
                break;
            }
            writer.write(&buf[..size]).unwrap();
        }
        if dignostic_failed_range.is_empty() {
            let _ = std::fs::remove_file(format!("{}.dignostic_failed.json", textures.name));
        } else {
            // try deledte dignostic file
            println!("[Dignostic] failed range: {:?}", dignostic_failed_range);
            let writer = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(format!("{}.dignostic_failed_range.json", textures.name))
                .expect("Failed to create file");
            let writer = std::io::BufWriter::new(writer);
            serde_json::to_writer(writer, &dignostic_failed_range).unwrap();
        }
    }
}

#[cfg(test)]
mod test {
    use regex::Regex;

    use super::SimpleTextOutput;
    use crate::{output::RegexUsage, OutputRegex};

    #[test]
    fn test_clear() {
        let output = SimpleTextOutput::new(vec![OutputRegex {
            usage: RegexUsage::Replace("".to_string()),
            regex: r"(翻译为[:：]\n?|^\d+\.\s?|是否违规.*)".to_string(),
        }]);
        let content = "翻译为:\n1. \"......\"\n2. 我非常兴奋。心跳加速，听到自己心脏跳动的声音。无论如何想要吸烟，于是在口袋里掏找着。\n3. 握住火柴盒的手指微微颤抖着。含上烟，小心地搓动火柴。火柴点燃了香烟，意识到烟深深地吸入。\n4. \"呼——\"\n5. 心静如水。充分体验错觉。然后我从豪华的皮革沙发上微微坐起，向右伸手，拿起桌子上的水晶球。\n6. 握着平球大小的小水晶球。慢慢地注入体内魔力，观影用的巨大窗户一下子就打开了。\n7. \"......\"\n8. 原来是一个明亮的黄色房间，明亮的黄色墙纸和宽广的地毯铺满了整个房间。\n是否违规: 否";
        println!("before \n{}", content);
        let content = output.clear(content);
        println!("after \n{}", content);
        let content = "(1) 在各种意义上，刊登了展示夫妻关系良好的短篇小说。\n是否违规：否\n\n(2) 顺带一提，在第24集之后，我想“年龄差25岁？”但是，她和桜井之间的年龄差为23岁。\n是否违规：否\n\n(3) 我不戴戒指是因为平日要做家务。\n是否违规：否\n\n(4) 由于外表和性格等原因，他在某些方面比女儿更受欢迎。是的，他是可爱本的对象。\n是否违规：否";
        let output = SimpleTextOutput::new(vec![
            OutputRegex {
                usage: RegexUsage::Replace("".to_string()),
                regex: r"(是否违规.+|\(\d+\)\s?)".to_string(),
            },
            OutputRegex {
                usage: RegexUsage::Replace("\n".to_string()),
                regex: r"\n{2,}".to_string(),
            },
        ]);
        println!("before \n{}", content);
        let content = output.clear(content);
        println!("after \n{}", content);
    }

    #[test]
    fn test_regex_capture() {
        let str = "翻译为: \n16681. 明天终于要决战了吗。\n16682. 明天所有的一切都会结束。\n16683. 不，这场战斗是为了安特拉和卡尔马尔的所有人，\n16684. 以及为了这个世界。\n16685. 然后，解开这个诅咒，\n16686. 回到林和赫尔维蒂亚。\n16687. 再次回到以前的生活。\n16688. 不，安特拉。\n16689. 我是……\n16690. 怎么了？这么晚干嘛。\n16691. 那是我的台词。\n16692. 你怎么了？在这样的地方。\n16693. 明天是决战，如果不早点休息的话\n16694. 有点紧张。\n16695. 是啊。即将到来。\n16696. 我也是一样。\n16697. 但是没问题的。\n16698. 我会保护爱德华先生的。\n16699. 嘿嘿，这样男子汉就没法站起来了吧？\n16700. 安特拉由我来保护。无论发生什么。\n16701. 好的。谢谢。\n16702. 最后的魔王了吧。\n16703. 有种终于走到这里来的感觉。\n16704. 不好意思，安特拉。\n16705. 把你卷入这样的战斗中。\n16706. 没事的。那个时候……\n16707. 正是因为那个，我得到了战斗的力量。\n16708. 能够为了保卫王国而战，\n16709. 都是因为遇见了爱德华先生他们。\n16710. 我是这样想的。\n16711. 喂，安特拉。\n16712. 我有话要对你说。\n16713. 谈话吗？是关于什么？\n16714. 不，是件大事。\n16715. 所以等这场战斗结束了再听我说可以吗？\n\n是否违规: 否";
        let regex = Regex::new(r"(\d+)\.\s?(.*)").unwrap();
        regex.captures_iter(str).for_each(|cap| {
            println!("{}: {}, cap: {:?}", &cap[1], &cap[2], cap);
        });
        let str = "
6. 他们的气息在森林深处

7. 然而要小心，也会感受到野生动物的气息。

8. 呵呵，这点小意思我也能轻而易举搞定。

9. 我用刀也非常在行。

10. 无论如何，我们走吧。

11. 首先，我们要进行游戏设定。

12. 本作为同人作品相当长，希望您能根据自己的游戏风格进行设定以获得更好的游戏体验。

13. 更改难度不会影响到剧情进程、事件或者掉落物品。

14. 难度：简单\\n战斗后解除战斗不能状况，恢复少量HP和MP。

15. 返回大基地时可以自动全回复。

16. Boss战前会出现全回复点。

17. 在更改武器或进行Boss战时，TP不会被重置。\\n如果您没有完成前一次游戏，也可以选择开始宽松模式。

18. 总的来说，这将会使剧情推进变得更加容易，

19. 尽管前往ＮＴＲ路线的危险性略微降低。

20. 设置难度为：简单

21. 难度：普通\\n战斗后仅解除战斗不能状态。

22. 在Boss战等事件开始时或更改武器时会重置TP。

23. 失败在Boss战中会出现全回复点。
";
        regex.captures_iter(str).for_each(|cap| {
            println!("{}: {}, cap: {:?}", &cap[1], &cap[2], cap);
        });
    }

    #[test]
    fn test_regex_capture_with_replace() {
        let str = "
(1) 对不起，真的对不起！请原谅我！
(2) 对不起，真的对不起
！请原谅我！
(3) 对不起，真的对不起！请原谅我！
(4) 对不起，真的对不起
！请原谅我！
(5) 对不起，真的对不起！请原谅我！
";
        println!("str {}", str);
        let regex = Regex::new(r"\n[^\n\(是]").unwrap();
        let res = regex.replace_all(str, "\\n").to_string();
        println!("res {}", res);
        let regex = Regex::new(r"\(\d+\)\s?(.+)").unwrap();
        regex.captures_iter(&res).for_each(|cap| {
            println!("line: {}", &cap[1]);
        });
        let str = "
翻译为:
(1) 对不起，真的对不起！请原谅我！
(2) 对不起，真的对不起！请原谅我！
(3) 对不起，真的对不起！请原谅我！
(4) 对不起，真的对不起！请原谅我！
(5) 对不起，真的对不起！请原谅我！

是否违规: 是
";
        println!("str {}", str);
        let regex = Regex::new(r"\n[^\n\(是]").unwrap();
        let res = regex.replace_all(str, "\\n").to_string();
        println!("res {}", res);
        let regex = Regex::new(r"\(\d+\)\s?(.+)").unwrap();
        regex.captures_iter(&res).for_each(|cap| {
            println!("line: {}", &cap[1]);
        });
    }
}
