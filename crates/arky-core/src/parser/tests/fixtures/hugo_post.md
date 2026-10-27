+++
title = "Shipping the parser"
date = 2026-10-03T09:15:00-04:00
lastmod = 2026-10-04T08:00:00Z
draft = false
weight = 10
tags = ["rust", "markdown", "parsing"]
summary = 'A literal string: no \escapes here'
params = { author = "Ada", toc = true, reading_time = 4.5 }
+++
The parser reads a file in two stages.

{{< figure src="/img/stages.png" title="The two stages" >}}

## Stage one

The lexer turns lines into tokens:

```go
type Token struct {
	Kind TokenKind
	Span Range
}
```

## Stage two

A grammar over those tokens builds the tree. See [the grammar]({{< ref "grammar.md" >}}).
