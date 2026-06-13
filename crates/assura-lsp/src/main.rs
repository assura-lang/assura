use tower_lsp::{LspService, Server};

use assura_lsp::AssuraLanguageServer;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(AssuraLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
