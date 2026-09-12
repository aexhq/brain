use brain::{
    ModelExecutor,
    model::{Dialect, ProviderRegistry, RemoteModelClient, RemoteModelConfig},
};
use brain_protocol::{
    ContentBlock, FileMediaType, Media, Message, ModelBinding, ModelRequest, Role, SessionId,
    ToolDefinition,
};
use clap::Parser;
use serde_json::json;

#[derive(Parser)]
struct Args {
    #[arg(long, env = "BRAIN_MEDIA_PROVIDERS_FILE")]
    providers_file: Option<std::path::PathBuf>,
    #[arg(long, env = "BRAIN_MEDIA_PROVIDER")]
    provider: String,
    #[arg(long, env = "BRAIN_MEDIA_MODEL")]
    model: String,
    #[arg(long, env = "BRAIN_MEDIA_API_KEY", hide_env_values = true)]
    api_key: String,
    #[arg(long, env = "BRAIN_MEDIA_IMAGE_URL", hide_env_values = true)]
    image_url: String,
    #[arg(long, env = "BRAIN_MEDIA_PDF_URL", hide_env_values = true)]
    pdf_url: String,
    #[arg(long, env = "BRAIN_MEDIA_COMPACTION")]
    compaction: bool,
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn text(value: &str) -> ContentBlock {
    ContentBlock::Text { text: value.into() }
}
fn user(content: Vec<ContentBlock>) -> Message {
    Message {
        role: Role::User,
        content,
    }
}

async fn call(
    client: &RemoteModelClient,
    binding: &ModelBinding,
    messages: Vec<Message>,
    tools: &[ToolDefinition],
    compact: bool,
) -> Result<Message> {
    let response = client
        .execute(
            &SessionId::new("media-probe"),
            binding,
            ModelRequest {
                messages,
                system: None,
                tools: None,
                response_format: None,
                max_output_tokens: if compact { None } else { Some(2048) },
                options: if compact {
                    [("operation".into(), json!("compact"))].into()
                } else {
                    Default::default()
                },
            },
            tools,
            &mut |_| {},
        )
        .await?;
    if response.stop_reason == brain_protocol::StopReason::MaxTokens {
        return Err("provider exhausted the probe output budget".into());
    }
    Ok(response.message)
}

fn verify(message: &Message) -> Result<()> {
    let answer = message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    if !answer.contains("RED=2") || !answer.contains("BLUE=3") {
        return Err(format!("provider did not identify both visual counts: {answer}").into());
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    run(&args).await.map_err(|error| {
        let mut message = error.to_string();
        for value in [&args.api_key, &args.image_url, &args.pdf_url] {
            if !value.is_empty() {
                message = message.replace(value, "[redacted]");
            }
        }
        message.into()
    })
}

async fn run(args: &Args) -> Result<()> {
    brain_protocol::validate_media_url(&args.image_url)?;
    brain_protocol::validate_media_url(&args.pdf_url)?;
    let providers = args
        .providers_file
        .as_ref()
        .map(|path| {
            Ok::<_, Box<dyn std::error::Error>>(serde_json::from_slice(&std::fs::read(path)?)?)
        })
        .transpose()?
        .unwrap_or_default();
    let registry = ProviderRegistry::compose(providers, &[])?;
    let provider = registry.get(&args.provider).ok_or("unsupported provider")?;
    let client = RemoteModelClient::new(RemoteModelConfig {
        base_url: provider.base_url.clone(),
        api_key: args.api_key.clone(),
        limits: Default::default(),
        dialect: provider.dialect,
    })?;
    let binding = ModelBinding {
        provider: args.provider.clone(),
        name: args.model.clone(),
    };
    let file = Media::File {
        media_type: FileMediaType::Pdf,
        url: args.pdf_url.clone(),
    };
    let prompt = "Count the red circles in the image and the blue squares in the PDF. Reply only RED=n BLUE=n with the observed counts.";
    let mut history = vec![user(vec![
        text(prompt),
        ContentBlock::Image {
            url: args.image_url.clone(),
        },
        file.clone().into(),
    ])];
    let answer = call(&client, &binding, history.clone(), &[], false).await?;
    verify(&answer)?;
    println!("{}: user image and PDF passed", binding.provider);
    let tools = [ToolDefinition {
        name: "read_report".into(),
        description: "Read the PDF containing the blue shapes.".into(),
        input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        output_schema: None,
    }];
    history = vec![user(vec![
        text(
            "Count the red circles in the image. Call read_report to obtain the PDF, then count its blue squares. After reading the Tool result, reply only RED=n BLUE=n.",
        ),
        ContentBlock::Image {
            url: args.image_url.clone(),
        },
    ])];
    let requested = call(&client, &binding, history.clone(), &tools, false).await?;
    let id = requested
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::ToolUse { id, name, .. } if name == "read_report" => Some(id.clone()),
            _ => None,
        })
        .ok_or("provider did not request the PDF Tool")?;
    history.push(requested);
    history.push(user(vec![ContentBlock::ToolResult {
        tool_use_id: id,
        content: json!(prompt),
        is_error: false,
        media: vec![file],
    }]));
    let answer = call(&client, &binding, history.clone(), &tools, false).await?;
    verify(&answer)?;
    println!(
        "{}: PDF supplied only by the Tool result passed",
        binding.provider
    );
    history.push(answer);
    history.push(user(vec![text(
        "Repeat the two observed counts as RED=n BLUE=n.",
    )]));
    let answer = call(&client, &binding, history.clone(), &tools, false).await?;
    verify(&answer)?;
    println!("{}: subsequent turn passed", binding.provider);
    history.push(answer);
    if args.compaction {
        if provider.dialect != Dialect::OpenAiResponses {
            return Err("compaction requires the Responses dialect".into());
        }
        // This probe explicitly selects visible context; gateway-encrypted reasoning is not portable to compaction.
        for message in &mut history {
            message
                .content
                .retain(|block| !matches!(block, ContentBlock::Native { .. }));
        }
        history.retain(|message| !message.content.is_empty());
        let compacted = call(&client, &binding, history, &[], true).await?;
        verify(
            &call(
                &client,
                &binding,
                vec![
                    compacted,
                    user(vec![text(
                        "Repeat the two observed counts as RED=n BLUE=n.",
                    )]),
                ],
                &[],
                false,
            )
            .await?,
        )?;
        println!(
            "{}: Responses compaction and subsequent turn passed",
            binding.provider
        );
    }
    println!(
        "{}: image, PDF, Tool-result media and continuation passed",
        binding.provider
    );
    Ok(())
}
