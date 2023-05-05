use std::{
    fmt::Debug,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::{
    select,
    sync::mpsc::{self, Sender},
};

use crate::{
    textures::{Textures, TranslatedLine},
    Configuration, Timer,
};

use super::chatgpt::{TokenizedBatchizer, TranslateChatGPT};

pub async fn translate(
    textures: Textures,
    textures_mut: &mut Textures,
    cfg: &Configuration,
) -> Result<()> {
    let textures_arc = Arc::new(textures);

    // handle ctrl-c
    let (close_tx, mut close_rx) = mpsc::channel::<i32>(1);
    let close_tx_c = close_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for event");
        if let Err(e) = close_tx_c.send(3).await {
            eprintln!("Failed to send close signal: {}", e);
        }
    });

    // handle translations
    let (tx, mut rx) = mpsc::channel::<TranslatedLine>(1);
    let textures_r = textures_arc.clone();
    let tx_r = tx.clone();
    let close_tx_r = close_tx.clone();
    let mut wait_for_translations = 0;
    if let Some(chatgpt_opt) = &cfg.chatgpt_opt {
        wait_for_translations += 1;
        let batchizer = TokenizedBatchizer {
            bep: tiktoken_rs::cl100k_base().unwrap(),
            max_tokens: cfg.batchizer_opt.max_tokens.clone(),
        };
        let mut chat_gpt = TranslateChatGPT::new(
            chatgpt_opt.clone(),
            cfg.specify_range.clone(),
            cfg.lang_from.to_name(),
            cfg.lang_to.to_name(),
        );
        tokio::spawn(async move {
            chat_gpt.translate(textures_r, batchizer, tx_r).await;
            if let Err(e) = close_tx_r.send(1).await {
                eprintln!("Failed to send close signal: {}", e);
            }
        });
    }
    // todo baidu, deepl

    let mut timer = Timer::new(std::time::Duration::from_secs(60)); // save every 60 seconds
    loop {
        select! {
            Some(line) = rx.recv() => {
                textures_mut.update(line);
                if timer.finished() {
                    textures_mut.save()?;
                }
            }
            Some(n) = close_rx.recv() => {
                wait_for_translations -= n;
            }
            else => {
                eprintln!("unexpected error in select!");
            }
        };
        if wait_for_translations <= 0 {
            textures_mut.save()?;
            break;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Translator {
    ChatGPT,
}

#[async_trait]
pub trait Translate<T> {
    async fn translate<F>(
        &mut self,
        text: Arc<Textures>,
        batchizer: F,
        sender: Sender<TranslatedLine>,
    ) where
        F: Batchizer<T>;
}

#[async_trait]
pub trait ConcurrentTranslate<T>: Translate<T> {
    type Client: TranslateClient<T>;
    fn create_batch_queue<F>(&self, batchizer: F, textures: &Textures) -> Vec<BatchPackage<T>>
    where
        F: Batchizer<T>;

    fn create_client(&mut self) -> Self::Client;
    fn max_concurrent(&self) -> i32;
}

#[async_trait]
impl<M, T> Translate<T> for M
where
    M: ConcurrentTranslate<T> + Send + Sync + 'static,
    T: Debug + Send + Sync + 'static,
{
    async fn translate<F>(
        &mut self,
        textures: Arc<Textures>,
        batchizer: F,
        sender: Sender<TranslatedLine>,
    ) where
        F: Batchizer<T>,
    {
        let batch_queue = self.create_batch_queue(batchizer, textures.as_ref());
        let batch_len = batch_queue.len();
        let batch_queue = Arc::new(Mutex::new(batch_queue));
        let (close_tx, mut close_rx) = mpsc::channel::<i32>(1);
        let max_concurrent = self.max_concurrent().min(batch_len as i32);
        println!(
            "start translate, batch len: {}, max concurrent {}",
            batch_len, max_concurrent
        );
        for t in 0..max_concurrent {
            let batch_queue = batch_queue.clone();
            let sender = sender.clone();
            let client = self.create_client();
            let close_tx = close_tx.clone();
            tokio::spawn(async move {
                let mut batch_and_range: Option<BatchPackage<T>> = None;
                loop {
                    if batch_and_range.is_none() {
                        let mut batch_queue = batch_queue.lock().unwrap();
                        batch_and_range = batch_queue.pop();
                        if batch_and_range.is_none() {
                            break;
                        }
                    }
                    let br = batch_and_range.as_ref().unwrap();
                    // println!("{} request: {}-{}", t, br.1 .0, br.1 .1);
                    let result = client.request(br).await;
                    match result {
                        Ok(translated) => {
                            println!(
                                "{} request: {}-{} total {}\n{:?}\n",
                                t,
                                br.1 .0,
                                br.1 .1,
                                br.1 .1 - br.1 .0 + 1,
                                br.0[0]
                            );
                            println!("{} response:\n{}\n", t, translated.content);
                            if let Err(err) = sender.send(translated).await {
                                println!("send change error: {:?}", err);
                            }
                            // set batch_and_range to None, so that we can pop a new batch from the queue
                            batch_and_range = None;
                        }
                        Err(err) => {
                            println!("{} request error: {:?}", t, err);
                            // keep batch_and_range not changed, so that it will be retried
                        }
                    }
                }
                close_tx.send(1).await.expect("close tx error");
            });
        }
        let mut wait_for_close = max_concurrent;
        loop {
            if wait_for_close <= 0 {
                break;
            }
            if let Some(i) = close_rx.recv().await {
                wait_for_close -= i;
            } else {
                println!("close rx error");
                break;
            }
        }
    }
}

pub type BatchPackage<T> = (Vec<T>, (usize, usize));

#[async_trait]
pub trait TranslateClient<T>: Send + Sync + 'static {
    async fn request(&self, batch_and_range: &BatchPackage<T>) -> Result<TranslatedLine>;
}

pub trait Batchizer<T>: Send + Sync + 'static {
    fn batchize(&self, textures: &Textures, index: usize, end: Option<usize>) -> (Vec<T>, usize);
}

#[cfg(test)]
mod test {
    use isolang::Language;
    use serde::{Deserialize, Serialize};

    #[test]
    fn test_iso_639() {
        let en = Language::from_639_1("en").expect(
            "not a valid iso 639-1 code see https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes",
        );
        assert_eq!(en.to_name(), "English");
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct Wrapper {
        pub lang: Language,
    }

    #[test]
    fn test_iso_639_serialize() {
        let en = Language::from_639_1("en").expect(
            "not a valid iso 639-1 code see https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes",
        );
        let wrapper = Wrapper { lang: en };
        let en_str = toml::to_string(&wrapper).unwrap();
        assert_eq!(en_str, "lang = \"eng\"\n");
    }

    #[test]
    fn test_iso_639_deserialize() {
        let en_str = "lang = \"en\"\n";
        let wrapper: Wrapper = toml::from_str(en_str).unwrap();
        let en = Language::from_639_1("en").expect(
            "not a valid iso 639-1 code see https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes",
        );
        assert_eq!(wrapper.lang, en);
        let en_str = "lang = \"eng\"\n";
        let wrapper: Wrapper = toml::from_str(en_str).unwrap();
        assert_eq!(wrapper.lang, en);
        let en_str = "lang = \"zho\"\n";
        let wrapper: Wrapper = toml::from_str(en_str).unwrap();
        assert_eq!(wrapper.lang.to_name(), "Chinese");
        let en_str = "lang = \"cn\"\n";
        let wrapper: Wrapper = toml::from_str(en_str).unwrap();
        assert_eq!(wrapper.lang.to_name(), "Chinese");
    }
}
