//! Scaffold templates for MCPB packages.

use crate::mcpb::{McpbTransport, PythonPackageManager};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Node.js scaffold files.
pub struct NodeScaffold {
    /// Content for server/index.js
    pub index_js: String,
    /// Content for package.json
    pub package_json: String,
}

/// Python scaffold files.
pub struct PythonScaffold {
    /// Content for server/main.py
    pub main_py: String,
    /// Content for project file (pyproject.toml or requirements.txt)
    pub project_file: String,
    /// Name of the project file
    pub project_file_name: &'static str,
}

/// Rust scaffold files.
pub struct RustScaffold {
    /// Content for src/main.rs
    pub main_rs: String,
    /// Content for src/lib.rs
    pub lib_rs: String,
    /// Content for Cargo.toml
    pub cargo_toml: String,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Generate Node.js scaffold files.
pub fn node_scaffold(name: &str, transport: McpbTransport) -> NodeScaffold {
    match transport {
        McpbTransport::Stdio => node_scaffold_stdio(name),
        McpbTransport::Http => node_scaffold_http(name),
    }
}

/// Generate Node.js stdio scaffold files.
fn node_scaffold_stdio(name: &str) -> NodeScaffold {
    let index_js = format!(
        r#"#!/usr/bin/env node

import {{ McpServer }} from "@modelcontextprotocol/sdk/server/mcp.js";
import {{ StdioServerTransport }} from "@modelcontextprotocol/sdk/server/stdio.js";
import {{ z }} from "zod";

const server = new McpServer({{
  name: "{name}",
  version: "0.1.0",
}});

const HelloOutputSchema = z.object({{
  message: z.string().describe("The greeting message"),
}});

server.registerTool(
  "hello",
  {{
    description: "Say hello",
    outputSchema: HelloOutputSchema,
  }},
  async () => {{
    const output = {{ message: "Hello from {name}!" }};
    return {{
      content: [{{ type: "text", text: JSON.stringify(output) }}],
      structuredContent: output,
    }};
  }}
);

const transport = new StdioServerTransport();
await server.connect(transport);

console.error("{name} MCP server running...");
"#
    );

    let package_json = format!(
        r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "type": "module",
  "scripts": {{
    "start": "node server/index.js"
  }},
  "dependencies": {{
    "@modelcontextprotocol/sdk": "^1.0.0",
    "zod": "^3.24.0"
  }}
}}
"#
    );

    NodeScaffold {
        index_js,
        package_json,
    }
}

/// Generate Node.js HTTP scaffold files.
fn node_scaffold_http(name: &str) -> NodeScaffold {
    let index_js = format!(
        r#"#!/usr/bin/env node

import {{ McpServer }} from "@modelcontextprotocol/sdk/server/mcp.js";
import {{ StreamableHTTPServerTransport }} from "@modelcontextprotocol/sdk/server/streamableHttp.js";
import {{ createServer }} from "http";
import {{ randomUUID }} from "crypto";
import {{ z }} from "zod";

const server = new McpServer({{
  name: "{name}",
  version: "0.1.0",
}});

const HelloOutputSchema = z.object({{
  message: z.string().describe("The greeting message"),
}});

server.registerTool(
  "hello",
  {{
    description: "Say hello",
    outputSchema: HelloOutputSchema,
  }},
  async () => {{
    const output = {{ message: "Hello from {name}!" }};
    return {{
      content: [{{ type: "text", text: JSON.stringify(output) }}],
      structuredContent: output,
    }};
  }}
);

const transports = {{}};

const httpServer = createServer(async (req, res) => {{
  const url = new URL(req.url, `http://${{req.headers.host}}`);

  if (url.pathname !== "/mcp") {{
    res.writeHead(404);
    res.end("Not Found");
    return;
  }}

  const sessionId = req.headers["mcp-session-id"];

  if (req.method === "POST") {{
    let transport = transports[sessionId];

    if (!transport) {{
      transport = new StreamableHTTPServerTransport({{
        sessionIdGenerator: () => randomUUID(),
        onsessioninitialized: (id) => {{
          transports[id] = transport;
        }},
      }});
      transport.onclose = () => {{
        if (transport.sessionId) delete transports[transport.sessionId];
      }};
      await server.connect(transport);
    }}

    let body = "";
    for await (const chunk of req) body += chunk;
    await transport.handleRequest(req, res, JSON.parse(body));
  }} else if (req.method === "GET") {{
    const transport = transports[sessionId];
    if (transport) {{
      await transport.handleRequest(req, res);
    }} else {{
      res.writeHead(400);
      res.end("No session");
    }}
  }} else if (req.method === "DELETE") {{
    const transport = transports[sessionId];
    if (transport) {{
      await transport.handleRequest(req, res);
      delete transports[sessionId];
    }} else {{
      res.writeHead(400);
      res.end("No session");
    }}
  }} else {{
    res.writeHead(405);
    res.end("Method Not Allowed");
  }}
}});

const portArg = process.argv.find((a) => a.startsWith("--port="));
const port = portArg ? parseInt(portArg.split("=")[1]) : 3000;

httpServer.listen(port, "127.0.0.1", () => {{
  console.error(`{name} running on http://127.0.0.1:${{port}}/mcp`);
}});
"#
    );

    let package_json = format!(
        r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "type": "module",
  "scripts": {{
    "start": "node server/index.js"
  }},
  "dependencies": {{
    "@modelcontextprotocol/sdk": "^1.0.0",
    "zod": "^3.24.0"
  }}
}}
"#
    );

    NodeScaffold {
        index_js,
        package_json,
    }
}

/// Generate Python scaffold files.
pub fn python_scaffold(
    name: &str,
    transport: McpbTransport,
    pkg_manager: PythonPackageManager,
) -> PythonScaffold {
    match transport {
        McpbTransport::Stdio => python_scaffold_stdio(name, pkg_manager),
        McpbTransport::Http => python_scaffold_http(name, pkg_manager),
    }
}

/// Generate Python stdio scaffold files.
fn python_scaffold_stdio(name: &str, pkg_manager: PythonPackageManager) -> PythonScaffold {
    let main_py = format!(
        r#"from pydantic import BaseModel, Field
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("{name}")


class HelloOutput(BaseModel):
    """Output structure for the hello tool."""

    message: str = Field(description="The greeting message")


@mcp.tool()
def hello() -> HelloOutput:
    """Say hello."""
    return HelloOutput(message="Hello from {name}!")


if __name__ == "__main__":
    mcp.run()
"#
    );

    let (project_file, project_file_name) = match pkg_manager {
        PythonPackageManager::Pip => ("mcp>=1.0.0\n".to_string(), "requirements.txt"),
        PythonPackageManager::Uv => (
            format!(
                r#"[project]
name = "{name}"
version = "0.1.0"
requires-python = ">=3.10"
dependencies = ["mcp>=1.0.0"]

[tool.uv]
dev-dependencies = []
"#
            ),
            "pyproject.toml",
        ),
        PythonPackageManager::Poetry => (
            format!(
                r#"[tool.poetry]
name = "{name}"
version = "0.1.0"
description = ""
authors = []
package-mode = false

[tool.poetry.dependencies]
python = "^3.10"
mcp = "^1.0.0"
"#
            ),
            "pyproject.toml",
        ),
    };

    PythonScaffold {
        main_py,
        project_file,
        project_file_name,
    }
}

/// Generate Python HTTP scaffold files.
fn python_scaffold_http(name: &str, pkg_manager: PythonPackageManager) -> PythonScaffold {
    let main_py = format!(
        r#"import argparse
import contextlib

from fastapi import FastAPI
from pydantic import BaseModel, Field
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("{name}", stateless_http=True, json_response=True)


class HelloOutput(BaseModel):
    """Output structure for the hello tool."""

    message: str = Field(description="The greeting message")


@mcp.tool()
def hello() -> HelloOutput:
    """Say hello."""
    return HelloOutput(message="Hello from {name}!")


@contextlib.asynccontextmanager
async def lifespan(app: FastAPI):
    async with contextlib.AsyncExitStack() as stack:
        await stack.enter_async_context(mcp.session_manager.run())
        yield


app = FastAPI(lifespan=lifespan)
app.mount("/", mcp.streamable_http_app())


if __name__ == "__main__":
    import uvicorn

    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=3000)
    args = parser.parse_args()

    uvicorn.run(app, host="127.0.0.1", port=args.port)
"#
    );

    let (project_file, project_file_name) = match pkg_manager {
        PythonPackageManager::Pip => (
            "mcp>=1.0.0\nfastapi>=0.100.0\nuvicorn>=0.20.0\n".to_string(),
            "requirements.txt",
        ),
        PythonPackageManager::Uv => (
            format!(
                r#"[project]
name = "{name}"
version = "0.1.0"
requires-python = ">=3.10"
dependencies = ["mcp>=1.0.0", "fastapi>=0.100.0", "uvicorn>=0.20.0"]

[tool.uv]
dev-dependencies = []
"#
            ),
            "pyproject.toml",
        ),
        PythonPackageManager::Poetry => (
            format!(
                r#"[tool.poetry]
name = "{name}"
version = "0.1.0"
description = ""
authors = []
package-mode = false

[tool.poetry.dependencies]
python = "^3.10"
mcp = "^1.0.0"
fastapi = "^0.100.0"
uvicorn = "^0.20.0"
"#
            ),
            "pyproject.toml",
        ),
    };

    PythonScaffold {
        main_py,
        project_file,
        project_file_name,
    }
}

/// Generate Rust scaffold files.
pub fn rust_scaffold(name: &str, transport: McpbTransport) -> RustScaffold {
    match transport {
        McpbTransport::Stdio => rust_scaffold_stdio(name),
        McpbTransport::Http => rust_scaffold_http(name),
    }
}

/// Generate Rust stdio scaffold files.
fn rust_scaffold_stdio(name: &str) -> RustScaffold {
    // Convert kebab-case to snake_case for Rust crate name
    let name_snake = name.replace('-', "_");

    let main_rs = format!(
        r#"use anyhow::Result;
use {name_snake}::Server;
use rmcp::{{ServiceExt, transport::stdio}};
use tracing_subscriber::{{self, EnvFilter}};

#[tokio::main]
async fn main() -> Result<()> {{
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let service = Server::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}}
"#
    );

    let lib_rs = format!(
        r#"use rmcp::{{
    ErrorData as McpError, Json, ServerHandler,
    handler::server::tool::ToolRouter,
    model::{{ServerCapabilities, ServerInfo, Implementation, ProtocolVersion}},
    tool, tool_router, tool_handler,
}};
use schemars::JsonSchema;
use serde::{{Deserialize, Serialize}};

/// Output structure for the hello tool.
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
pub struct HelloOutput {{
    /// The greeting message.
    pub message: String,
}}

#[derive(Clone)]
pub struct Server {{
    tool_router: ToolRouter<Self>,
}}

#[tool_router]
impl Server {{
    pub fn new() -> Self {{
        Self {{
            tool_router: Self::tool_router(),
        }}
    }}

    #[tool(description = "Say hello")]
    fn hello(&self) -> Result<Json<HelloOutput>, McpError> {{
        Ok(Json(HelloOutput {{
            message: "Hello from {name}!".to_string(),
        }}))
    }}
}}

#[tool_handler]
impl ServerHandler for Server {{
    fn get_info(&self) -> ServerInfo {{
        ServerInfo {{
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: None,
        }}
    }}
}}
"#
    );

    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"

[dependencies]
rmcp = {{ version = "0.12", features = ["server", "macros", "transport-io"] }}
tokio = {{ version = "1", features = ["macros", "rt-multi-thread", "io-std"] }}
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
schemars = "1"
anyhow = "1.0"
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}
"#
    );

    RustScaffold {
        main_rs,
        lib_rs,
        cargo_toml,
    }
}

/// Generate Rust HTTP scaffold files.
fn rust_scaffold_http(name: &str) -> RustScaffold {
    // Convert kebab-case to snake_case for Rust crate name
    let name_snake = name.replace('-', "_");

    let main_rs = format!(
        r#"use anyhow::Result;
use clap::Parser;
use {name_snake}::Server;
use rmcp::transport::streamable_http_server::{{
    StreamableHttpService, session::local::LocalSessionManager,
}};
use tracing_subscriber::{{self, EnvFilter}};

#[derive(Parser)]
struct Args {{
    #[arg(long, default_value = "3000")]
    port: u16,
}}

#[tokio::main]
async fn main() -> Result<()> {{
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let args = Args::parse();

    let service = StreamableHttpService::new(
        || Ok(Server::new()),
        LocalSessionManager::default().into(),
        Default::default(),
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    let addr = format!("127.0.0.1:{{}}", args.port);
    let tcp_listener = tokio::net::TcpListener::bind(&addr).await?;

    eprintln!("{name} running on http://{{}}/mcp", addr);

    axum::serve(tcp_listener, router)
        .with_graceful_shutdown(async {{
            tokio::signal::ctrl_c().await.unwrap();
        }})
        .await?;

    Ok(())
}}
"#
    );

    let lib_rs = format!(
        r#"use rmcp::{{
    ErrorData as McpError, Json, ServerHandler,
    handler::server::tool::ToolRouter,
    model::{{ServerCapabilities, ServerInfo, Implementation, ProtocolVersion}},
    tool, tool_router, tool_handler,
}};
use schemars::JsonSchema;
use serde::{{Deserialize, Serialize}};

/// Output structure for the hello tool.
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
pub struct HelloOutput {{
    /// The greeting message.
    pub message: String,
}}

#[derive(Clone)]
pub struct Server {{
    tool_router: ToolRouter<Self>,
}}

#[tool_router]
impl Server {{
    pub fn new() -> Self {{
        Self {{
            tool_router: Self::tool_router(),
        }}
    }}

    #[tool(description = "Say hello")]
    fn hello(&self) -> Result<Json<HelloOutput>, McpError> {{
        Ok(Json(HelloOutput {{
            message: "Hello from {name}!".to_string(),
        }}))
    }}
}}

#[tool_handler]
impl ServerHandler for Server {{
    fn get_info(&self) -> ServerInfo {{
        ServerInfo {{
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: None,
        }}
    }}
}}
"#
    );

    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"

[dependencies]
rmcp = {{ version = "0.12", features = ["server", "macros", "transport-streamable-http-server"] }}
tokio = {{ version = "1", features = ["macros", "rt-multi-thread", "net", "signal"] }}
axum = "0.8"
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
schemars = "1"
anyhow = "1.0"
clap = {{ version = "4", features = ["derive"] }}
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}
"#
    );

    RustScaffold {
        main_rs,
        lib_rs,
        cargo_toml,
    }
}

/// Generate .mcpbignore content for Rust projects.
pub fn rust_mcpbignore_template() -> &'static str {
    r#"# OS files
.DS_Store
Thumbs.db

# Editor/IDE
.idea/
.vscode/
*.swp
*.swo

# Git
.git/
.gitignore

# MCPB bundles
*.mcpb

# Rust source files
src/
Cargo.lock

# Rust build artifacts (ignore everything except release and debug binaries)
target/
!target/release/**
!target/debug/**
"#
}

/// Generate .mcpbignore content (same for all types).
pub fn mcpbignore_template() -> &'static str {
    r#"# OS files
.DS_Store
Thumbs.db

# Editor/IDE
.idea/
.vscode/
*.swp
*.swo

# Git
.git/
.gitignore

# MCPB bundles
*.mcpb
"#
}

/// Generate .gitignore content for Node.js projects.
pub fn node_gitignore_template() -> &'static str {
    "node_modules/\n*.mcpb\n"
}

/// Generate .gitignore content for Python projects.
pub fn python_gitignore_template() -> &'static str {
    ".venv/\n__pycache__/\n*.mcpb\n"
}

/// Generate .gitignore content for Rust projects.
pub fn rust_gitignore_template() -> &'static str {
    "target/\n*.mcpb\n"
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_scaffold_stdio() {
        let scaffold = node_scaffold("my-tool", McpbTransport::Stdio);
        assert!(scaffold.index_js.contains("my-tool"));
        assert!(scaffold.index_js.contains("@modelcontextprotocol/sdk"));
        assert!(scaffold.index_js.contains("StdioServerTransport"));
        assert!(scaffold.index_js.contains("outputSchema"));
        assert!(scaffold.index_js.contains("structuredContent"));
        assert!(scaffold.index_js.contains("z.object"));
        assert!(scaffold.package_json.contains("\"name\": \"my-tool\""));
        assert!(scaffold.package_json.contains("@modelcontextprotocol/sdk"));
        assert!(scaffold.package_json.contains("zod"));
    }

    #[test]
    fn test_node_scaffold_http() {
        let scaffold = node_scaffold("my-tool", McpbTransport::Http);
        assert!(scaffold.index_js.contains("my-tool"));
        assert!(scaffold.index_js.contains("@modelcontextprotocol/sdk"));
        assert!(scaffold.index_js.contains("StreamableHTTPServerTransport"));
        assert!(scaffold.index_js.contains("/mcp"));
        assert!(scaffold.index_js.contains("outputSchema"));
        assert!(scaffold.index_js.contains("structuredContent"));
        assert!(scaffold.index_js.contains("z.object"));
        assert!(scaffold.package_json.contains("\"name\": \"my-tool\""));
        assert!(scaffold.package_json.contains("zod"));
    }

    #[test]
    fn test_python_scaffold_stdio_uv() {
        let scaffold = python_scaffold("my-tool", McpbTransport::Stdio, PythonPackageManager::Uv);
        assert!(scaffold.main_py.contains("my-tool"));
        assert!(scaffold.main_py.contains("FastMCP"));
        assert!(scaffold.main_py.contains("mcp.run()"));
        assert!(scaffold.main_py.contains("BaseModel"));
        assert!(scaffold.main_py.contains("HelloOutput"));
        assert!(scaffold.main_py.contains("-> HelloOutput"));
        assert_eq!(scaffold.project_file_name, "pyproject.toml");
        assert!(scaffold.project_file.contains("name = \"my-tool\""));
        assert!(scaffold.project_file.contains("mcp>=1.0.0"));
        assert!(scaffold.project_file.contains("[tool.uv]"));
    }

    #[test]
    fn test_python_scaffold_stdio_pip() {
        let scaffold = python_scaffold("my-tool", McpbTransport::Stdio, PythonPackageManager::Pip);
        assert!(scaffold.main_py.contains("my-tool"));
        assert!(scaffold.main_py.contains("BaseModel"));
        assert!(scaffold.main_py.contains("HelloOutput"));
        assert_eq!(scaffold.project_file_name, "requirements.txt");
        assert!(scaffold.project_file.contains("mcp>=1.0.0"));
    }

    #[test]
    fn test_python_scaffold_stdio_poetry() {
        let scaffold = python_scaffold(
            "my-tool",
            McpbTransport::Stdio,
            PythonPackageManager::Poetry,
        );
        assert!(scaffold.main_py.contains("my-tool"));
        assert!(scaffold.main_py.contains("BaseModel"));
        assert!(scaffold.main_py.contains("HelloOutput"));
        assert_eq!(scaffold.project_file_name, "pyproject.toml");
        assert!(scaffold.project_file.contains("[tool.poetry]"));
        assert!(scaffold.project_file.contains("package-mode = false"));
    }

    #[test]
    fn test_python_scaffold_http_uv() {
        let scaffold = python_scaffold("my-tool", McpbTransport::Http, PythonPackageManager::Uv);
        assert!(scaffold.main_py.contains("my-tool"));
        assert!(scaffold.main_py.contains("FastMCP"));
        assert!(scaffold.main_py.contains("streamable_http_app"));
        assert!(scaffold.main_py.contains("uvicorn"));
        assert!(scaffold.main_py.contains("BaseModel"));
        assert!(scaffold.main_py.contains("HelloOutput"));
        assert!(scaffold.main_py.contains("-> HelloOutput"));
        assert_eq!(scaffold.project_file_name, "pyproject.toml");
        assert!(scaffold.project_file.contains("fastapi"));
        assert!(scaffold.project_file.contains("uvicorn"));
    }

    #[test]
    fn test_python_scaffold_http_pip() {
        let scaffold = python_scaffold("my-tool", McpbTransport::Http, PythonPackageManager::Pip);
        assert!(scaffold.main_py.contains("uvicorn"));
        assert!(scaffold.main_py.contains("BaseModel"));
        assert!(scaffold.main_py.contains("HelloOutput"));
        assert_eq!(scaffold.project_file_name, "requirements.txt");
        assert!(scaffold.project_file.contains("fastapi"));
        assert!(scaffold.project_file.contains("uvicorn"));
    }

    #[test]
    fn test_mcpbignore() {
        let content = mcpbignore_template();
        assert!(content.contains(".DS_Store"));
        assert!(content.contains(".git/"));
    }

    #[test]
    fn test_rust_scaffold_stdio() {
        let scaffold = rust_scaffold("my-tool", McpbTransport::Stdio);
        assert!(scaffold.main_rs.contains("my_tool::Server"));
        assert!(scaffold.main_rs.contains("rmcp"));
        assert!(scaffold.main_rs.contains("stdio()"));
        assert!(scaffold.lib_rs.contains("Hello from my-tool!"));
        assert!(scaffold.lib_rs.contains("ServerHandler"));
        assert!(scaffold.lib_rs.contains("Json<HelloOutput>"));
        assert!(scaffold.lib_rs.contains("JsonSchema"));
        assert!(scaffold.lib_rs.contains("HelloOutput"));
        assert!(scaffold.cargo_toml.contains("name = \"my-tool\""));
        assert!(scaffold.cargo_toml.contains("transport-io"));
        assert!(scaffold.cargo_toml.contains("schemars"));
    }

    #[test]
    fn test_rust_scaffold_http() {
        let scaffold = rust_scaffold("my-tool", McpbTransport::Http);
        assert!(scaffold.main_rs.contains("my_tool::Server"));
        assert!(scaffold.main_rs.contains("StreamableHttpService"));
        assert!(scaffold.main_rs.contains("/mcp"));
        assert!(scaffold.lib_rs.contains("Hello from my-tool!"));
        assert!(scaffold.lib_rs.contains("Json<HelloOutput>"));
        assert!(scaffold.lib_rs.contains("JsonSchema"));
        assert!(scaffold.lib_rs.contains("HelloOutput"));
        assert!(
            scaffold
                .cargo_toml
                .contains("transport-streamable-http-server")
        );
        assert!(scaffold.cargo_toml.contains("axum"));
        assert!(scaffold.cargo_toml.contains("schemars"));
    }

    #[test]
    fn test_rust_mcpbignore() {
        let content = rust_mcpbignore_template();
        assert!(content.contains(".DS_Store"));
        assert!(content.contains(".git/"));
        assert!(content.contains("Cargo.lock"));
        assert!(content.contains("target/"));
        assert!(content.contains("!target/release/**"));
        assert!(content.contains("!target/debug/**"));
        assert!(content.contains("src/"));
    }
}
