use std::{fs, io::BufReader, str::FromStr};

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tiktoken_rs::CoreBPE;

use crate::textures::{TextureLine, Textures, TranslatedLine};

use super::translator::{
    BatchPackage, Batchizer, ConcurrentTranslate, TranslateClient, Translator,
};

pub struct TokenizedBatchizer {
    pub bep: CoreBPE,
    pub max_tokens: usize,
}

impl Batchizer<ChatCompletionMessage> for TokenizedBatchizer {
    fn batchize(&self, textures: &Textures, start: usize) -> (Vec<ChatCompletionMessage>, usize) {
        let mut str_content = String::new();
        let mut max_tokens = 0;
        let mut size = 0;
        let mut prefix: Option<char> = None;
        let mut i = start;
        while i < textures.lines.len() {
            let line = &textures.lines[i];
            max_tokens += self.bep.encode_with_special_tokens(&line.content).len();
            let prefix_a = line.content.chars().next();
            let is_same_suffix = prefix_a == prefix;
            if !is_same_suffix {
                prefix = prefix_a;
            }
            if !is_same_suffix && max_tokens > self.max_tokens && !str_content.is_empty() {
                break;
            }
            str_content.push_str(&format!("({}) {}\n", i - start + 1, &line.content));
            i += 1;
            size += 1;
        }
        (
            vec![ChatCompletionMessage::new(
                ChatCompletionRole::User,
                &str_content,
            )],
            size,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatGPTAPI {
    pub api_key: String,
    pub api_url: String,
    pub org_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatGPTOptions {
    pub api_pool: Vec<ChatGPTAPI>,
    pub prompt_path: Option<String>,
    pub max_concurrent: i32,
}

pub struct TranslateChatGPT {
    pub specify_range: Option<Vec<(usize, usize)>>,
    pub api_pool: Vec<ChatGPTAPI>,
    pub prompt_path: Option<String>,
    pub max_concurrent: i32,
    client_count: usize,
    prompts: Option<Vec<ChatCompletionMessage>>,
}

impl TranslateChatGPT {
    pub fn new(opt: ChatGPTOptions, specify_range: Option<Vec<(usize, usize)>>) -> Self {
        if opt.api_pool.is_empty() {
            panic!("ChatGPT api pool is empty");
        }
        let prompts = if let Some(path) = &opt.prompt_path {
            let prompt_file = fs::OpenOptions::new()
                .read(true)
                .open(path)
                .expect("ChatGPT prompt file not found");
            let reader = BufReader::new(prompt_file);
            let prompts = serde_json::from_reader::<_, Vec<ChatCompletionMessage>>(reader)
                .expect("ChatGPT prompt file is not valid");
            Some(prompts)
        } else {
            None
        };
        Self {
            specify_range,
            api_pool: opt.api_pool,
            prompt_path: opt.prompt_path,
            max_concurrent: opt.max_concurrent,
            client_count: 0,
            prompts,
        }
    }
}

fn line_count_batchized(
    textures: &Textures,
    specify_range: &Option<Vec<(usize, usize)>>,
) -> Vec<BatchPackage<ChatCompletionMessage>> {
    let mut batch_queue: Vec<BatchPackage<ChatCompletionMessage>> = Vec::new();
    let lines = &textures.lines;
    if let Some(specify_range) = specify_range {
        for (start, end) in specify_range.iter() {
            let mut str_content = String::new();
            let max_size = 4;
            let mut size = 0;
            for i in *start..=*end {
                size += 1;
                let line = &lines[i];
                str_content.push_str(&format!("{}. {}\n", size + 1, &line.content));
                if size == max_size || i == *end {
                    // println!("add: {} i {}", add, i);
                    batch_queue.push((
                        vec![ChatCompletionMessage::new(
                            ChatCompletionRole::User,
                            &str_content,
                        )],
                        (i + 1 - size, i),
                    ));
                    str_content.clear();
                    size = 0;
                }
            }
        }
        // reverse for pop
        batch_queue.reverse();
    }
    batch_queue
}

#[async_trait]
impl ConcurrentTranslate<ChatCompletionMessage> for TranslateChatGPT {
    type Client = ChatGPTClient;

    fn create_batch_queue<F>(
        &self,
        batchizer: F,
        textures: &Textures,
    ) -> Vec<BatchPackage<ChatCompletionMessage>>
    where
        F: Batchizer<ChatCompletionMessage>,
    {
        let by_line_count = false; //todo
        if !by_line_count {
            let mut batch_queue = Vec::new();
            let mut spec_range_index = 0;
            let mut i = if let Some(specify_range) = &self.specify_range {
                specify_range[spec_range_index].0
            } else {
                textures.curr_index
            };
            while i < textures.lines.len() {
                let (batch, size) = batchizer.batchize(textures, i);
                batch_queue.push((batch, (i, i + size - 1)));
                i = if let Some(spec_range) = &self.specify_range {
                    spec_range_index += 1;
                    if spec_range_index >= spec_range.len() {
                        break;
                    }
                    spec_range[spec_range_index].0
                } else {
                    i + size
                };
            }
            // reverse for pop
            batch_queue.reverse();
            batch_queue
        } else {
            line_count_batchized(textures, &self.specify_range)
        }
    }

    fn create_client(&mut self) -> Self::Client {
        let api = &self.api_pool[self.client_count % self.api_pool.len()];
        self.client_count += 1;
        ChatGPTClient::new(
            &api.api_key,
            &api.api_url,
            self.prompts.clone(),
            api.org_id.clone(),
        )
    }

    fn max_concurrent(&self) -> i32 {
        self.max_concurrent
    }
}

#[derive(Clone)]
pub struct ChatGPTClient {
    pub client: reqwest::Client,
    pub api_key: String,
    pub api_url: String,
    pub org_id: Option<String>,
    pub timeout: std::time::Duration,
    pub proxy: Option<reqwest::Proxy>,
    pub request: ChatCompletionRequest,
}

#[async_trait]
impl TranslateClient<ChatCompletionMessage> for ChatGPTClient {
    async fn request(
        &self,
        batch_and_range: &BatchPackage<ChatCompletionMessage>,
    ) -> Result<TranslatedLine> {
        let (batch, range) = batch_and_range;
        let resp = self.create_chat_completion(batch.clone()).await?;
        let resp_message = resp.choices.into_iter().next().unwrap().message;
        Ok(TranslatedLine::new(
            Translator::ChatGPT,
            resp_message.content.clone(),
            range.0,
            range.1,
        ))
    }
}

impl ChatGPTClient {
    pub fn new(
        api_key: &str,
        api_url: &str,
        prompts: Option<Vec<ChatCompletionMessage>>,
        org_id: Option<String>,
    ) -> Self {
        // check api_key
        if api_key.is_empty() {
            panic!("api_key is empty");
        }
        // check api_url
        if api_url.is_empty() {
            panic!("api_url is empty");
        }
        let timeout = std::time::Duration::from_secs(60 * 3);
        let client = reqwest::ClientBuilder::new()
            .timeout(timeout)
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                let mut api_key = api_key.to_string();
                api_key.insert_str(0, "Bearer ");
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    reqwest::header::HeaderValue::from_str(&api_key).unwrap(),
                );
                if let Some(org_id) = org_id.as_ref() {
                    headers.insert(
                        reqwest::header::HeaderName::from_str("OpenAI-Organization").unwrap(),
                        reqwest::header::HeaderValue::from_str(org_id).unwrap(),
                    );
                }
                headers.insert(
                    reqwest::header::CONTENT_TYPE,
                    reqwest::header::HeaderValue::from_str("application/json").unwrap(),
                );
                headers.insert(
                    reqwest::header::ACCEPT,
                    reqwest::header::HeaderValue::from_str("application/json").unwrap(),
                );
                headers
            })
            .build()
            .unwrap();

        // request
        let mut request = ChatCompletionRequest::default();
        if let Some(prompts) = prompts {
            request.messages = prompts;
        }
        Self {
            client,
            api_key: api_key.to_string(),
            api_url: api_url.to_string(),
            org_id,
            request,
            timeout,
            proxy: None,
        }
    }

    pub async fn create_chat_completion(
        &self,
        messages: Vec<ChatCompletionMessage>,
    ) -> Result<ChatCompletionResponse> {
        let mut request = self.request.clone();
        request.messages.extend(messages);
        // println!("messages :{:?}", request.messages);
        let resp = self
            .client
            .post(&self.api_url)
            .body(&request)
            .send()
            .await?;
        let status = resp.status();
        match resp.bytes().await {
            Ok(bs) => match serde_json::from_slice(&bs) {
                Ok(completion) => Ok(completion),
                Err(e) => {
                    println!(
                        "status: {}, decode response error: {}",
                        status,
                        String::from_utf8(bs.to_vec()).unwrap()
                    );
                    Err(e.into())
                }
            },
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatCompletionMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatCompletionMessage {
    pub role: ChatCompletionRole,
    pub content: String,
}

impl ChatCompletionMessage {
    pub fn new(role: ChatCompletionRole, content: &str) -> Self {
        Self {
            role,
            content: content.to_string(),
        }
    }
}

impl From<&mut TextureLine> for Vec<ChatCompletionMessage> {
    fn from(line: &mut TextureLine) -> Self {
        let mut messages = Vec::new();
        messages.push(ChatCompletionMessage::new(
            ChatCompletionRole::User,
            &line.content,
        ));
        if let Some(translation) = line
            .translated
            .iter()
            .find(|t| t.translator == Translator::ChatGPT)
        {
            messages.push(ChatCompletionMessage::new(
                ChatCompletionRole::Assistant,
                translation.content.as_str(),
            ));
        }
        messages
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ChatCompletionRole {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
}

impl AsRef<str> for ChatCompletionRole {
    fn as_ref(&self) -> &str {
        match self {
            ChatCompletionRole::System => "system",
            ChatCompletionRole::User => "user",
            ChatCompletionRole::Assistant => "assistant",
        }
    }
}

impl Default for ChatCompletionRequest {
    fn default() -> Self {
        Self {
            model: "gpt-3.5-turbo".to_string(),
            messages: Vec::new(),
            temperature: None,
            top_p: None,
            n: None,
            max_tokens: None,
            presence_penalty: None,
            frequency_penalty: None,
            stream: Some(false),
            stop: None,
            user: None,
        }
    }
}

// impl Into<Body> for &ChatCompletionRequest
impl Into<reqwest::Body> for &ChatCompletionRequest {
    fn into(self) -> reqwest::Body {
        let json = serde_json::to_string(&self).unwrap();
        reqwest::Body::from(json)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub choices: Vec<ChatCompletionChoice>,
    pub usage: ChatComplectionUsage,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ChatCompletionChoice {
    pub index: u32,
    pub message: ChatCompletionMessage,
    pub finish_reason: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ChatComplectionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[cfg(test)]
mod test {

    use std::io;

    use super::*;

    #[test]
    pub fn test_chat_completion_role_serialize() {
        let role = ChatCompletionRole::User;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"user\"");
    }

    #[test]
    pub fn test_chat_completion_message_serialize() {
        let message = ChatCompletionMessage {
            role: ChatCompletionRole::User,
            content: "test".to_string(),
        };
        let json = serde_json::to_string(&message).unwrap();
        assert_eq!(json, "{\"role\":\"user\",\"content\":\"test\"}");
    }

    #[test]
    pub fn test_chat_completion_message_deserialize() {
        let json = "{\"role\":\"user\",\"content\":\"test\"}";
        let message: ChatCompletionMessage = serde_json::from_str(json).unwrap();
        assert_eq!(message.role, ChatCompletionRole::User);
        assert_eq!(message.content, "test");
    }

    #[test]
    pub fn test_chat_completion_request_serialize() {
        let request = ChatCompletionRequest {
            model: "test".to_string(),
            messages: Vec::new(),
            temperature: None,
            top_p: None,
            n: None,
            stream: None,
            stop: None,
            max_tokens: None,
            presence_penalty: None,
            frequency_penalty: None,
            user: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(json, "{\"model\":\"test\",\"messages\":[]}");
    }

    #[test]
    pub fn test_chat_completion_request_deserialize() {
        let json = "{\"model\":\"test\",\"messages\":[]}";
        let request: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.model, "test");
        assert_eq!(request.messages.len(), 0);
        assert_eq!(request.user, None);
    }

    #[test]
    pub fn test_chat_completion_response_serialize() {
        let response = ChatCompletionResponse {
            id: "test".to_string(),
            object: "test".to_string(),
            created: 0,
            choices: Vec::new(),
            usage: ChatComplectionUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(
            json,
            "{\"id\":\"test\",\"object\":\"test\",\"created\":0,\"choices\":[],\"usage\":{\"prompt_tokens\":0,\"completion_tokens\":0,\"total_tokens\":0}}"
        );
    }

    #[test]
    pub fn test_chat_completion_response_deserialize() {
        let json = "
        { 
            \"id\": \"chatcmpl-123\", 
            \"object\": \"chat.completion\", 
            \"created\": 1677652288, 
            \"choices\": [ 
                { 
                \"index\": 0, 
                \"message\": { 
                    \"role\": \"assistant\", 
                    \"content\": \"Hello there, how may I assist you today?\" 
                    }, 
                \"finish_reason\": \"stop\" 
                } 
            ], 
            \"usage\": { 
                \"prompt_tokens\": 9, 
                \"completion_tokens\": 12, 
                \"total_tokens\": 21 
            } 
        } 
        ";
        println!("json: \n{}", json);
        let response: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id, "chatcmpl-123");
        assert_eq!(response.object, "chat.completion");
        assert_eq!(response.created, 1677652288);
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.usage.prompt_tokens, 9);
        assert_eq!(response.usage.completion_tokens, 12);
        assert_eq!(response.usage.total_tokens, 21);
    }

    #[test]
    pub fn test_tokenized_prompt() {
        let prompt = "You are a helpful assistant that only speaks French.\nHello, how are you?\nParlez-vous francais?";
        let bep = tiktoken_rs::cl100k_base().unwrap();
        let len = bep.encode_with_special_tokens(prompt).len();
        println!("cl100k base len: {}", len);
        let bep = tiktoken_rs::p50k_base().unwrap();
        let len = bep.encode_with_special_tokens(prompt).len();
        println!("p50k base len: {}", len);
    }

    #[test]
    pub fn test_tokenized_batchizer() {
        let lines = vec![
            "请原谅我",
            "请原谅我",
            "请原谅我",
            "请原谅我",
            " 请原谅我",
            "请原谅我",
            "请原谅我",
            "请原谅我",
        ]
        .iter()
        .map(|s| TextureLine::new(0, 0, s.to_string(), false))
        .collect::<Vec<_>>();
        let textures = Textures {
            lines,
            curr_index: 0,
            name: "".to_string(),
        };

        let mut batchizer = TokenizedBatchizer {
            bep: tiktoken_rs::cl100k_base().unwrap(),
            max_tokens: 500,
        };
        let (_, size) = batchizer.batchize(&textures, 0);
        assert_eq!(size, 8);
        batchizer.max_tokens = 1;
        let (_, size) = batchizer.batchize(&textures, 0);
        assert_eq!(size, 4);
    }

    #[test]
    pub fn test_chat_gpt_create_client() {
        let mut gpt = TranslateChatGPT::new(
            ChatGPTOptions {
                api_pool: vec![
                    ChatGPTAPI {
                        api_key: "test1".to_string(),
                        api_url: "test1.html".to_string(),
                        org_id: None,
                    },
                    ChatGPTAPI {
                        api_key: "test2".to_string(),
                        api_url: "test2.html".to_string(),
                        org_id: None,
                    },
                    ChatGPTAPI {
                        api_key: "test3".to_string(),
                        api_url: "test1.html".to_string(),
                        org_id: None,
                    },
                ],
                prompt_path: None,
                max_concurrent: 10,
            },
            None,
        );
        let client = gpt.create_client();
        assert_eq!(client.api_key, "test1");
        assert_eq!(client.api_url, "test1.html");
        let client = gpt.create_client();
        assert_eq!(client.api_key, "test2");
        assert_eq!(client.api_url, "test2.html");
        let client = gpt.create_client();
        assert_eq!(client.api_key, "test3");
        assert_eq!(client.api_url, "test1.html");
        let client = gpt.create_client();
        assert_eq!(client.api_key, "test1");
        assert_eq!(client.api_url, "test1.html");
        let client = gpt.create_client();
        assert_eq!(client.api_key, "test2");
        assert_eq!(client.api_url, "test2.html");
    }

    #[tokio::test]
    pub async fn test_chat_completion_adult_content() {
        let api_key: Option<&'static str> = option_env!("OPENAI_API_KEY");
        let api_url: Option<&'static str> = option_env!("OPENAI_API_URL");
        let content = "
「お客さま、此処にはお連れ様はおられません。恥ずかしい言葉を言っても大丈夫でございます」
「……」
あッ……、お姉さんの唇が、微笑みの形に変わった……。
「わ、私のおまんこに……、お前の長くて、太いちんぽ……、ハメて、くれ……」
お姉さんは知らず知らず、微笑んでる。エッチな言葉を震える声で言ってた。
「よろしゅうございますとも」
ずにゅうぅぅ……
「あぁぁッ……、きたぁッ……」
お父さんはニヤリと笑い、待ち兼ねたようにちんぽをおまんこに突き挿れた。おまんこを押し開いてちんぽが侵入していく。お姉さんは、嬉しそうに微笑んでた。
ギッ、ギッ、ギッ、ギッ……
「あ、あぁッ……、よくも、私に言わせてくれたな……」
天井が軋む。お姉さんは、お父さんに恨めしげに言ってた。でも、怒ってはいないみたい。
「これも、マッサージの一環でございます……」
ギッ、ギッ、ギッ、ギッ……
「あッ、あッ、あぁ……、そんな……」
ぼくもわかってきた。お父さんは、エッチな事を何でもマッサージって言ってるみたい。
「それに、お客さまも興奮しておいででした……」
ギッ、ギッ、ギッ、ギッ……";
        println!("content: {}", content);
        let messages = vec![ChatCompletionMessage::new(
            super::ChatCompletionRole::User,
            content,
        )];
        let client = TranslateChatGPT::new(
            ChatGPTOptions {
                api_pool: vec![ChatGPTAPI {
                    api_key: api_key.unwrap().to_string(),
                    api_url: api_url.unwrap().to_string(),
                    org_id: None,
                }],
                prompt_path: Some("./assets/prompt_violation_1.json".to_string()),
                max_concurrent: 1,
            },
            None,
        )
        .create_client();

        let response = client.create_chat_completion(messages).await.unwrap();
        println!("response: {:?}", response);

        let messages = vec![ChatCompletionMessage::new(
            super::ChatCompletionRole::User,
            content,
        )];
        let client = TranslateChatGPT::new(
            ChatGPTOptions {
                api_pool: vec![ChatGPTAPI {
                    api_key: api_key.unwrap().to_string(),
                    api_url: api_url.unwrap().to_string(),
                    org_id: None,
                }],
                prompt_path: Some("./assets/prompt_violation_3.json".to_string()),
                max_concurrent: 1,
            },
            None,
        )
        .create_client();

        let response = client.create_chat_completion(messages).await.unwrap();
        println!("response: {:?}", response);
    }

    #[test]
    fn test_chat_completion_message_deserialize_from_file() {
        let file = fs::OpenOptions::new()
            .read(true)
            .open("./assets/prompt_violation_1.json")
            .unwrap();
        let reader = io::BufReader::new(file);
        let messages: Vec<ChatCompletionMessage> = serde_json::from_reader(reader).unwrap();
        messages
            .iter()
            .for_each(|m| println!("message: role {:?}\n{}", m.role, m.content));
    }

    #[test]
    fn test_batchizer_by_line_count_by_specify_range() {
        let lines = vec![
            "请原谅我1",
            "请原谅我2",
            "请原谅我3",
            "请原谅我4",
            "请原谅我5",
            "请原谅我6",
            "请原谅我7",
            "请原谅我8",
            "请原谅我9",
            "请原谅我10",
            "请原谅我11",
            "请原谅我12",
            "请原谅我13",
            "请原谅我14",
            "请原谅我15",
            "请原谅我16",
            "请原谅我17",
            "请原谅我18",
            "请原谅我19",
            "请原谅我20",
            "请原谅我21",
            "请原谅我22",
            "请原谅我23",
            "请原谅我24",
            "请原谅我25",
        ]
        .iter()
        .map(|s| TextureLine::new(0, 0, s.to_string(), false))
        .collect::<Vec<_>>();
        let textures = Textures {
            lines,
            curr_index: 0,
            name: "".to_string(),
        };

        let specify_range = vec![(0, 1), (2, 10), (21, 23)];
        let batch_queue = line_count_batchized(&textures, &Some(specify_range));
        let mut result = batch_queue
            .iter()
            .map(|b| (b.1 .0, b.1 .1))
            .collect::<Vec<(usize, usize)>>();
        result.reverse();
        assert_eq!(result, vec![(0, 1), (2, 5), (6, 9), (10, 10), (21, 23)]);
    }
}
