use lymals::{
    ast::{
        AstFile, Directive, Document, DocumentItem, LetBinding, Mapping, MappingEntry, Node,
        Scalar, ScalarKind, Sequence, SequenceItem, TagNode,
    },
    capabilities, server,
    symbols::{DocumentSymbolNode, build_document_symbols},
    syntax::{FileId, SourceSpan, SourceText},
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};
use tower_lsp::Server;
use tower_lsp::lsp_types::{ClientCapabilities, Url};

#[test]
fn document_symbol_provider_is_advertised() {
    let capabilities = capabilities::negotiate(&ClientCapabilities::default());
    assert_eq!(
        capabilities.document_symbol_provider,
        Some(tower_lsp::lsp_types::OneOf::Left(true))
    );
}

#[test]
fn document_symbol_tree_snapshot_captures_hierarchy_and_ranges() {
    let file_id = FileId(11);
    let text = concat!(
        "---\n",
        "@lyma 1\n",
        "@import \"./shared.lyma\" as shared\n",
        "@include \"./partials/base.lyma\"\n",
        "let region = \"us-east-1\"\n",
        "services:\n",
        "- !lambda worker\n",
        "schema: @Widget\n",
        "---\n",
        "catalog:\n",
        "- !Ref shared\n",
    );
    let source = SourceText::new(file_id, "fixture.lyma", text);
    let line_starts = line_starts(text);
    let ast = AstFile {
        span: span(file_id, 0, text.len()),
        documents: vec![
            Document {
                span: span(file_id, line_starts[0], line_end(text, &line_starts, 7)),
                separator_span: Some(line_span(file_id, text, &line_starts, 0)),
                items: vec![
                    DocumentItem::Directive(Directive {
                        name: "@lyma".to_string(),
                        span: line_span(file_id, text, &line_starts, 1),
                        value: Some("1".to_string()),
                    }),
                    DocumentItem::Directive(Directive {
                        name: "@import".to_string(),
                        span: line_span(file_id, text, &line_starts, 2),
                        value: Some("\"./shared.lyma\" as shared".to_string()),
                    }),
                    DocumentItem::Directive(Directive {
                        name: "@include".to_string(),
                        span: line_span(file_id, text, &line_starts, 3),
                        value: Some("\"./partials/base.lyma\"".to_string()),
                    }),
                    DocumentItem::Let(LetBinding {
                        name: "region".to_string(),
                        span: line_span(file_id, text, &line_starts, 4),
                        value_span: Some(span_of(text, file_id, "\"us-east-1\"", 1)),
                    }),
                    DocumentItem::Node(Node::Mapping(Mapping {
                        span: span(file_id, line_starts[5], line_end(text, &line_starts, 7)),
                        entries: vec![
                            MappingEntry {
                                key: "services".to_string(),
                                key_span: span_of(text, file_id, "services", 1),
                                span: line_span(file_id, text, &line_starts, 5),
                                metadata: None,
                                value: Some(Box::new(Node::Sequence(Sequence {
                                    span: line_span(file_id, text, &line_starts, 6),
                                    items: vec![SequenceItem {
                                        span: line_span(file_id, text, &line_starts, 6),
                                        value: Some(Box::new(Node::Tag(TagNode {
                                            name: "lambda".to_string(),
                                            span: line_span(file_id, text, &line_starts, 6),
                                            value: Some(Box::new(Node::Scalar(Scalar {
                                                kind: ScalarKind::Plain,
                                                span: span_of(text, file_id, "worker", 1),
                                                text: "worker".to_string(),
                                            }))),
                                        }))),
                                    }],
                                }))),
                            },
                            MappingEntry {
                                key: "schema".to_string(),
                                key_span: span_of(text, file_id, "schema", 1),
                                span: line_span(file_id, text, &line_starts, 7),
                                metadata: Some("Widget".to_string()),
                                value: Some(Box::new(Node::Scalar(Scalar {
                                    kind: ScalarKind::Plain,
                                    span: span_of(text, file_id, "@Widget", 1),
                                    text: "@Widget".to_string(),
                                }))),
                            },
                        ],
                    })),
                ],
            },
            Document {
                span: span(file_id, line_starts[8], text.len()),
                separator_span: Some(line_span(file_id, text, &line_starts, 8)),
                items: vec![DocumentItem::Node(Node::Mapping(Mapping {
                    span: span(file_id, line_starts[9], text.len()),
                    entries: vec![MappingEntry {
                        key: "catalog".to_string(),
                        key_span: span_of(text, file_id, "catalog", 1),
                        span: line_span(file_id, text, &line_starts, 9),
                        metadata: None,
                        value: Some(Box::new(Node::Sequence(Sequence {
                            span: line_span(file_id, text, &line_starts, 10),
                            items: vec![SequenceItem {
                                span: line_span(file_id, text, &line_starts, 10),
                                value: Some(Box::new(Node::Tag(TagNode {
                                    name: "Ref".to_string(),
                                    span: line_span(file_id, text, &line_starts, 10),
                                    value: Some(Box::new(Node::Scalar(Scalar {
                                        kind: ScalarKind::Plain,
                                        span: span_of(text, file_id, "shared", 2),
                                        text: "shared".to_string(),
                                    }))),
                                }))),
                            }],
                        }))),
                    }],
                }))],
            },
        ],
    };

    let actual = serde_json::to_string_pretty(&render_symbols(
        &build_document_symbols(&ast, &source),
        &source,
    ))
    .unwrap();

    insta::assert_snapshot!(actual, @r###"
    [
      {
        "children": [
          {
            "detail": "1",
            "kind": "Namespace",
            "name": "@lyma",
            "range": {
              "end": {
                "character": 7,
                "line": 1
              },
              "start": {
                "character": 0,
                "line": 1
              }
            },
            "selectionRange": {
              "end": {
                "character": 5,
                "line": 1
              },
              "start": {
                "character": 0,
                "line": 1
              }
            }
          },
          {
            "children": [
              {
                "detail": "@import",
                "kind": "File",
                "name": "./shared.lyma",
                "range": {
                  "end": {
                    "character": 33,
                    "line": 2
                  },
                  "start": {
                    "character": 0,
                    "line": 2
                  }
                },
                "selectionRange": {
                  "end": {
                    "character": 22,
                    "line": 2
                  },
                  "start": {
                    "character": 9,
                    "line": 2
                  }
                }
              },
              {
                "detail": "@import",
                "kind": "Variable",
                "name": "shared",
                "range": {
                  "end": {
                    "character": 33,
                    "line": 2
                  },
                  "start": {
                    "character": 0,
                    "line": 2
                  }
                },
                "selectionRange": {
                  "end": {
                    "character": 33,
                    "line": 2
                  },
                  "start": {
                    "character": 27,
                    "line": 2
                  }
                }
              }
            ],
            "detail": "\"./shared.lyma\" as shared",
            "kind": "Namespace",
            "name": "@import",
            "range": {
              "end": {
                "character": 33,
                "line": 2
              },
              "start": {
                "character": 0,
                "line": 2
              }
            },
            "selectionRange": {
              "end": {
                "character": 7,
                "line": 2
              },
              "start": {
                "character": 0,
                "line": 2
              }
            }
          },
          {
            "children": [
              {
                "detail": "@include",
                "kind": "File",
                "name": "./partials/base.lyma",
                "range": {
                  "end": {
                    "character": 31,
                    "line": 3
                  },
                  "start": {
                    "character": 0,
                    "line": 3
                  }
                },
                "selectionRange": {
                  "end": {
                    "character": 30,
                    "line": 3
                  },
                  "start": {
                    "character": 10,
                    "line": 3
                  }
                }
              }
            ],
            "detail": "\"./partials/base.lyma\"",
            "kind": "Namespace",
            "name": "@include",
            "range": {
              "end": {
                "character": 31,
                "line": 3
              },
              "start": {
                "character": 0,
                "line": 3
              }
            },
            "selectionRange": {
              "end": {
                "character": 8,
                "line": 3
              },
              "start": {
                "character": 0,
                "line": 3
              }
            }
          },
          {
            "kind": "Variable",
            "name": "region",
            "range": {
              "end": {
                "character": 24,
                "line": 4
              },
              "start": {
                "character": 0,
                "line": 4
              }
            },
            "selectionRange": {
              "end": {
                "character": 10,
                "line": 4
              },
              "start": {
                "character": 4,
                "line": 4
              }
            }
          },
          {
            "children": [
              {
                "children": [
                  {
                    "kind": "Operator",
                    "name": "!lambda",
                    "range": {
                      "end": {
                        "character": 16,
                        "line": 6
                      },
                      "start": {
                        "character": 0,
                        "line": 6
                      }
                    },
                    "selectionRange": {
                      "end": {
                        "character": 9,
                        "line": 6
                      },
                      "start": {
                        "character": 3,
                        "line": 6
                      }
                    }
                  }
                ],
                "kind": "Array",
                "name": "[0]",
                "range": {
                  "end": {
                    "character": 16,
                    "line": 6
                  },
                  "start": {
                    "character": 0,
                    "line": 6
                  }
                },
                "selectionRange": {
                  "end": {
                    "character": 16,
                    "line": 6
                  },
                  "start": {
                    "character": 0,
                    "line": 6
                  }
                }
              }
            ],
            "kind": "Key",
            "name": "services",
            "range": {
              "end": {
                "character": 9,
                "line": 5
              },
              "start": {
                "character": 0,
                "line": 5
              }
            },
            "selectionRange": {
              "end": {
                "character": 8,
                "line": 5
              },
              "start": {
                "character": 0,
                "line": 5
              }
            }
          },
          {
            "children": [
              {
                "detail": "metadata",
                "kind": "Class",
                "name": "@Widget",
                "range": {
                  "end": {
                    "character": 15,
                    "line": 7
                  },
                  "start": {
                    "character": 0,
                    "line": 7
                  }
                },
                "selectionRange": {
                  "end": {
                    "character": 15,
                    "line": 7
                  },
                  "start": {
                    "character": 9,
                    "line": 7
                  }
                }
              }
            ],
            "kind": "Key",
            "name": "schema",
            "range": {
              "end": {
                "character": 15,
                "line": 7
              },
              "start": {
                "character": 0,
                "line": 7
              }
            },
            "selectionRange": {
              "end": {
                "character": 6,
                "line": 7
              },
              "start": {
                "character": 0,
                "line": 7
              }
            }
          }
        ],
        "kind": "Module",
        "name": "document1",
        "range": {
          "end": {
            "character": 15,
            "line": 7
          },
          "start": {
            "character": 0,
            "line": 0
          }
        },
        "selectionRange": {
          "end": {
            "character": 3,
            "line": 0
          },
          "start": {
            "character": 0,
            "line": 0
          }
        }
      },
      {
        "children": [
          {
            "children": [
              {
                "children": [
                  {
                    "kind": "Operator",
                    "name": "!Ref",
                    "range": {
                      "end": {
                        "character": 13,
                        "line": 10
                      },
                      "start": {
                        "character": 0,
                        "line": 10
                      }
                    },
                    "selectionRange": {
                      "end": {
                        "character": 6,
                        "line": 10
                      },
                      "start": {
                        "character": 3,
                        "line": 10
                      }
                    }
                  }
                ],
                "kind": "Array",
                "name": "[0]",
                "range": {
                  "end": {
                    "character": 13,
                    "line": 10
                  },
                  "start": {
                    "character": 0,
                    "line": 10
                  }
                },
                "selectionRange": {
                  "end": {
                    "character": 13,
                    "line": 10
                  },
                  "start": {
                    "character": 0,
                    "line": 10
                  }
                }
              }
            ],
            "kind": "Key",
            "name": "catalog",
            "range": {
              "end": {
                "character": 8,
                "line": 9
              },
              "start": {
                "character": 0,
                "line": 9
              }
            },
            "selectionRange": {
              "end": {
                "character": 7,
                "line": 9
              },
              "start": {
                "character": 0,
                "line": 9
              }
            }
          }
        ],
        "kind": "Module",
        "name": "document2",
        "range": {
          "end": {
            "character": 0,
            "line": 11
          },
          "start": {
            "character": 0,
            "line": 8
          }
        },
        "selectionRange": {
          "end": {
            "character": 3,
            "line": 8
          },
          "start": {
            "character": 0,
            "line": 8
          }
        }
      }
    ]
    "###);
}

#[tokio::test(flavor = "current_thread")]
async fn lsp_document_symbols_snapshot_returns_nested_symbols() {
    let (client_to_server, server_stdin) = tokio::io::duplex(16 * 1024);
    let (server_stdout, server_to_client) = tokio::io::duplex(16 * 1024);
    let (service, socket) = server::service();
    let server_task = tokio::spawn(async move {
        Server::new(server_stdin, server_stdout, socket)
            .serve(service)
            .await;
    });

    let mut writer = client_to_server;
    let mut reader = server_to_client;
    let uri = Url::parse("file:///document-symbols.lyma").unwrap();

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "capabilities": {}
            }
        }),
    )
    .await;
    let initialize = read_message(&mut reader).await;
    assert_eq!(
        initialize["result"]["capabilities"]["documentSymbolProvider"],
        true
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    )
    .await;
    let _initialized_log = read_message(&mut reader).await;

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "lyma",
                    "version": 1,
                    "text": "---\n@import \"./shared.lyma\" as shared\n@include \"./partials/base.lyma\"\nlet region = \"us-east-1\"\n!Ref shared\n"
                }
            }
        }),
    )
    .await;
    let _diagnostics = read_message(&mut reader).await;

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/documentSymbol",
            "params": {
                "textDocument": { "uri": uri }
            }
        }),
    )
    .await;
    let response = read_message(&mut reader).await;
    let actual = serde_json::to_string_pretty(&response["result"]).unwrap();

    insta::assert_snapshot!(actual, @r###"
    [
      {
        "children": [
          {
            "children": [
              {
                "detail": "@import",
                "kind": 1,
                "name": "./shared.lyma",
                "range": {
                  "end": {
                    "character": 33,
                    "line": 1
                  },
                  "start": {
                    "character": 0,
                    "line": 1
                  }
                },
                "selectionRange": {
                  "end": {
                    "character": 22,
                    "line": 1
                  },
                  "start": {
                    "character": 9,
                    "line": 1
                  }
                }
              },
              {
                "detail": "@import",
                "kind": 13,
                "name": "shared",
                "range": {
                  "end": {
                    "character": 33,
                    "line": 1
                  },
                  "start": {
                    "character": 0,
                    "line": 1
                  }
                },
                "selectionRange": {
                  "end": {
                    "character": 33,
                    "line": 1
                  },
                  "start": {
                    "character": 27,
                    "line": 1
                  }
                }
              }
            ],
            "detail": "\"./shared.lyma\" as shared",
            "kind": 3,
            "name": "@import",
            "range": {
              "end": {
                "character": 33,
                "line": 1
              },
              "start": {
                "character": 0,
                "line": 1
              }
            },
            "selectionRange": {
              "end": {
                "character": 7,
                "line": 1
              },
              "start": {
                "character": 0,
                "line": 1
              }
            }
          },
          {
            "children": [
              {
                "detail": "@include",
                "kind": 1,
                "name": "./partials/base.lyma",
                "range": {
                  "end": {
                    "character": 31,
                    "line": 2
                  },
                  "start": {
                    "character": 0,
                    "line": 2
                  }
                },
                "selectionRange": {
                  "end": {
                    "character": 30,
                    "line": 2
                  },
                  "start": {
                    "character": 10,
                    "line": 2
                  }
                }
              }
            ],
            "detail": "\"./partials/base.lyma\"",
            "kind": 3,
            "name": "@include",
            "range": {
              "end": {
                "character": 31,
                "line": 2
              },
              "start": {
                "character": 0,
                "line": 2
              }
            },
            "selectionRange": {
              "end": {
                "character": 8,
                "line": 2
              },
              "start": {
                "character": 0,
                "line": 2
              }
            }
          },
          {
            "kind": 13,
            "name": "region",
            "range": {
              "end": {
                "character": 24,
                "line": 3
              },
              "start": {
                "character": 0,
                "line": 3
              }
            },
            "selectionRange": {
              "end": {
                "character": 10,
                "line": 3
              },
              "start": {
                "character": 4,
                "line": 3
              }
            }
          },
          {
            "kind": 25,
            "name": "!Ref",
            "range": {
              "end": {
                "character": 11,
                "line": 4
              },
              "start": {
                "character": 0,
                "line": 4
              }
            },
            "selectionRange": {
              "end": {
                "character": 4,
                "line": 4
              },
              "start": {
                "character": 1,
                "line": 4
              }
            }
          }
        ],
        "kind": 2,
        "name": "document1",
        "range": {
          "end": {
            "character": 11,
            "line": 4
          },
          "start": {
            "character": 0,
            "line": 1
          }
        },
        "selectionRange": {
          "end": {
            "character": 3,
            "line": 0
          },
          "start": {
            "character": 0,
            "line": 0
          }
        }
      }
    ]
    "###);

    shutdown_server(&mut writer, &mut reader, server_task).await;
}

fn render_symbols(symbols: &[DocumentSymbolNode], source: &SourceText) -> Value {
    Value::Array(
        symbols
            .iter()
            .map(|symbol| render_symbol(symbol, source))
            .collect(),
    )
}

fn render_symbol(symbol: &DocumentSymbolNode, source: &SourceText) -> Value {
    let mut value = json!({
        "name": symbol.name,
        "kind": format!("{:?}", symbol.kind),
        "range": render_span(source, symbol.span),
        "selectionRange": render_span(source, symbol.selection_span),
    });

    if let Some(detail) = &symbol.detail {
        value["detail"] = Value::String(detail.clone());
    }

    if !symbol.children.is_empty() {
        value["children"] = Value::Array(
            symbol
                .children
                .iter()
                .map(|child| render_symbol(child, source))
                .collect(),
        );
    }

    value
}

fn render_span(source: &SourceText, span: SourceSpan) -> Value {
    json!({
        "start": render_position(source, span.start),
        "end": render_position(source, span.end),
    })
}

fn render_position(source: &SourceText, offset: usize) -> Value {
    let position = source.position(offset);
    json!({
        "line": position.line - 1,
        "character": position.column - 1,
    })
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

fn line_end(text: &str, starts: &[usize], line: usize) -> usize {
    let end = starts.get(line + 1).copied().unwrap_or(text.len());
    end.saturating_sub(1)
}

fn line_span(file_id: FileId, text: &str, starts: &[usize], line: usize) -> SourceSpan {
    span(file_id, starts[line], line_end(text, starts, line))
}

fn span(file_id: FileId, start: usize, end: usize) -> SourceSpan {
    SourceSpan::new(file_id, start, end)
}

fn span_of(text: &str, file_id: FileId, needle: &str, occurrence: usize) -> SourceSpan {
    let start = nth_offset(text, needle, occurrence);
    span(file_id, start, start + needle.len())
}

fn nth_offset(text: &str, needle: &str, occurrence: usize) -> usize {
    assert!(occurrence > 0);
    let mut search_from = 0;
    for index in 1..=occurrence {
        let found = text[search_from..].find(needle).unwrap();
        let absolute = search_from + found;
        if index == occurrence {
            return absolute;
        }
        search_from = absolute + needle.len();
    }
    unreachable!()
}

async fn shutdown_server(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut tokio::io::DuplexStream,
    server_task: tokio::task::JoinHandle<()>,
) {
    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "shutdown"
        }),
    )
    .await;
    let _shutdown_response = read_message(reader).await;
    let _shutdown_log = read_message(reader).await;

    send_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "exit"
        }),
    )
    .await;
    writer.shutdown().await.unwrap();
    timeout(Duration::from_secs(2), server_task)
        .await
        .expect("server task timed out")
        .expect("server task failed");
}

async fn send_message(stream: &mut tokio::io::DuplexStream, value: &Value) {
    let body = value.to_string();
    let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    stream.write_all(message.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
}

async fn read_message(stream: &mut tokio::io::DuplexStream) -> Value {
    let mut headers = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte).await.unwrap();
        headers.push(byte[0]);
        if headers.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let headers = String::from_utf8(headers).unwrap();
    let content_length = headers
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .unwrap()
        .parse::<usize>()
        .unwrap();

    let mut body = vec![0; content_length];
    stream.read_exact(&mut body).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}
