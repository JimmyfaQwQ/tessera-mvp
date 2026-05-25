use tessera_ast::*;
use tessera_lexer::{Spanned, Token, TokenStream};

use crate::error::ParseError;

pub struct Parser {
    tokens: TokenStream,
    pos: usize,
    errors: Vec<ParseError>,
    /// True when we're parsing a type annotation (disambiguates `<`).
    in_type_ctx: bool,
    /// Stack of currently-open delimiters with the span of their opener, used to
    /// blame the unclosed opener when a matching close token is missing.
    open_delims: Vec<(Token, Span)>,
}

impl Parser {
    pub fn new(tokens: TokenStream) -> Self {
        Self { tokens, pos: 0, errors: Vec::new(), in_type_ctx: false, open_delims: Vec::new() }
    }

    pub fn into_errors(self) -> Vec<ParseError> {
        self.errors
    }

    // ── Token helpers ─────────────────────────────────────────────────────────

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|s| &s.node)
    }

    #[allow(dead_code)]
    fn peek2(&self) -> Option<&Token> {
        self.tokens.get(self.pos + 1).map(|s| &s.node)
    }

    fn current_span(&self) -> Span {
        match self.tokens.get(self.pos) {
            Some(s) => s.span,
            // At EOF point at the end of the last token (a zero-width span) so
            // carets land at end-of-input rather than the start of the file.
            None => {
                let end = self.tokens.last().map(|s| s.span.end).unwrap_or(0);
                Span::new(end, end)
            }
        }
    }

    fn prev_span(&self) -> Span {
        if self.pos == 0 { Span::dummy() }
        else { self.tokens[self.pos - 1].span }
    }

    fn advance(&mut self) -> &Spanned<Token> {
        // Track open/close delimiters so we can point at an unclosed opener.
        let consumed = self.tokens.get(self.pos).map(|s| (s.node.clone(), s.span));
        if let Some((node, span)) = consumed {
            match node {
                Token::BraceOpen | Token::ParenOpen | Token::BracketOpen | Token::DollarBraceOpen => {
                    self.open_delims.push((node, span));
                }
                // Only pop when the closer matches the most recent opener, so a
                // stray/mismatched closer cannot desynchronize the stack and
                // blame the wrong opener. (`${ ... }` is closed by `}`.)
                Token::BraceClose
                    if matches!(
                        self.open_delims.last().map(|(t, _)| t),
                        Some(Token::BraceOpen) | Some(Token::DollarBraceOpen)
                    ) =>
                {
                    self.open_delims.pop();
                }
                Token::ParenClose
                    if matches!(self.open_delims.last().map(|(t, _)| t), Some(Token::ParenOpen)) =>
                {
                    self.open_delims.pop();
                }
                Token::BracketClose
                    if matches!(self.open_delims.last().map(|(t, _)| t), Some(Token::BracketOpen)) =>
                {
                    self.open_delims.pop();
                }
                _ => {}
            }
        }
        let tok = &self.tokens[self.pos];
        if self.pos < self.tokens.len() { self.pos += 1; }
        tok
    }

    fn describe_got(&self) -> String {
        match self.peek() {
            Some(t) => t.describe(),
            None => "end of file".to_string(),
        }
    }

    fn expect(&mut self, expected: &Token) -> Span {
        if self.peek() == Some(expected) {
            return self.advance().span;
        }

        let span = self.current_span();
        let at_eof = self.peek().is_none();

        // ── Root-cause analysis for the most common syntax mistakes ──────────
        let err = match expected {
            // A missing `;` is almost always meant to terminate the *previous*
            // statement, so point the caret right after it.
            Token::Semicolon => {
                let p = self.prev_span();
                let caret = Span::new(p.end, p.end);
                ParseError::new("missing `;` after statement", caret)
                    .primary_label("insert `;` here")
                    .with_help("statements in Tessera must be terminated with `;`")
            }
            // A missing closing delimiter: blame the unclosed opener.
            Token::BraceClose | Token::ParenClose | Token::BracketClose => {
                let mut e = ParseError::new(
                    format!("expected {}, but found {}", expected.describe(), self.describe_got()),
                    span,
                )
                .primary_label(format!("expected {} here", expected.describe()));
                if at_eof {
                    if let Some((open_tok, open_span)) = self.open_delims.last().cloned() {
                        e = e
                            .with_secondary(format!("unclosed {} opened here", open_tok.describe()), open_span)
                            .with_help(format!("add a matching {} to close this block", expected.describe()));
                    }
                }
                e
            }
            _ => ParseError::new(
                format!("expected {}, but found {}", expected.describe(), self.describe_got()),
                span,
            )
            .primary_label(format!("expected {} here", expected.describe())),
        };

        self.errors.push(err);
        span
    }

    fn eat(&mut self, tok: &Token) -> bool {
        if self.peek() == Some(tok) { self.advance(); true } else { false }
    }

    fn expect_ident(&mut self) -> Ident {
        let span = self.current_span();
        match self.peek().cloned() {
            Some(Token::Ident(name)) => { self.advance(); Ident::new(name, span) }
            _ => {
                self.errors.push(
                    ParseError::new(
                        format!("expected an identifier, but found {}", self.describe_got()),
                        span,
                    )
                    .primary_label("expected a name here"),
                );
                Ident::new("<error>", span)
            }
        }
    }

    fn at_eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    // ── Program ───────────────────────────────────────────────────────────────

    pub fn parse_program(&mut self) -> Program {
        let start = self.current_span();
        let mut items = Vec::new();
        while !self.at_eof() {
            match self.peek() {
                Some(Token::KwAtTemplate) => {
                    items.push(TopLevelItem::ScopeTemplateDecl(self.parse_scope_template_decl()));
                }
                Some(Token::KwDollarTemplate) => {
                    // `$template Name` is a named template declaration.
                    // `$template(` without a name is an anonymous inline thread spawn.
                    if matches!(self.tokens.get(self.pos + 1).map(|s| &s.node), Some(Token::Ident(_))) {
                        items.push(TopLevelItem::ThreadTemplateDecl(self.parse_thread_template_decl()));
                    } else {
                        items.push(TopLevelItem::Statement(self.parse_stmt()));
                    }
                }
                Some(Token::KwFunction) => {
                    self.advance();
                    let name = self.expect_ident();
                    items.push(TopLevelItem::FuncDef(self.finish_func_def(FuncKind::Sync, name)));
                }
                Some(Token::KwAsync) => {
                    self.advance();
                    self.expect(&Token::KwFunction);
                    let name = self.expect_ident();
                    items.push(TopLevelItem::FuncDef(self.finish_func_def(FuncKind::Async, name)));
                }
                _ => {
                    items.push(TopLevelItem::Statement(self.parse_stmt()));
                }
            }
        }
        let end = self.prev_span();
        Program { items, span: start.merge(end) }
    }

    // ── Scope template ────────────────────────────────────────────────────────

    fn parse_scope_template_decl(&mut self) -> ScopeTemplateDecl {
        let start = self.expect(&Token::KwAtTemplate);
        let name = if matches!(self.peek(), Some(Token::Ident(_))) {
            Some(self.expect_ident())
        } else {
            None
        };
        let params = self.parse_param_list_opt();
        let (members, end) = self.parse_scope_template_body();
        ScopeTemplateDecl { name, params, members, span: start.merge(end) }
    }

    fn parse_scope_template_body(&mut self) -> (Vec<ScopeTemplateMember>, Span) {
        let mut members = Vec::new();
        self.expect(&Token::BraceOpen);
        while !matches!(self.peek(), Some(Token::BraceClose) | None) {
            let m = self.parse_scope_template_member();
            members.push(m);
        }
        let end = self.expect(&Token::BraceClose);
        (members, end)
    }

    fn parse_scope_template_member(&mut self) -> ScopeTemplateMember {
        if matches!(self.peek(), Some(Token::KwDefine)) {
            self.advance();
            let decl = self.parse_expose_decl();
            return ScopeTemplateMember::Define(decl);
        }
        let is_async = self.eat(&Token::KwAsync);
        self.expect(&Token::KwFunction);
        let name = self.expect_ident();
        let func = self.finish_func_def(if is_async { FuncKind::Async } else { FuncKind::Sync }, name);
        match func.name.name.as_str() {
            "__on_enter__" => ScopeTemplateMember::OnEnter(func),
            "__on_exit__"  => ScopeTemplateMember::OnExit(func),
            _              => ScopeTemplateMember::MemberFunc(func),
        }
    }

    // ── Thread template ───────────────────────────────────────────────────────

    fn parse_thread_template_decl(&mut self) -> ThreadTemplateDecl {
        let start = self.expect(&Token::KwDollarTemplate);
        let name = if matches!(self.peek(), Some(Token::Ident(_))) {
            Some(self.expect_ident())
        } else {
            None
        };
        let params = self.parse_param_list_opt();
        let (members, end) = self.parse_thread_template_body();
        ThreadTemplateDecl { name, params, members, span: start.merge(end) }
    }

    fn parse_thread_template_body(&mut self) -> (Vec<ThreadTemplateMember>, Span) {
        let mut members = Vec::new();
        self.expect(&Token::BraceOpen);
        while !matches!(self.peek(), Some(Token::BraceClose) | None) {
            let m = self.parse_thread_template_member();
            members.push(m);
        }
        let end = self.expect(&Token::BraceClose);
        (members, end)
    }

    fn parse_thread_template_member(&mut self) -> ThreadTemplateMember {
        match self.peek().cloned() {
            Some(Token::KwExpose) => {
                self.advance();
                let decl = self.parse_expose_decl();
                ThreadTemplateMember::Expose(decl)
            }
            Some(Token::KwExposeMutable) => {
                self.advance();
                let decl = self.parse_expose_decl();
                ThreadTemplateMember::ExposeMutable(decl)
            }
            Some(Token::KwDefine) => {
                self.advance();
                let decl = self.parse_expose_decl();
                ThreadTemplateMember::Define(decl)
            }
            Some(Token::KwAsync) => {
                self.advance();
                match self.peek().cloned() {
                    Some(Token::KwHandler) => {
                        self.advance();
                        let h = self.parse_handler_def();
                        ThreadTemplateMember::Handler(h)
                    }
                    Some(Token::KwFunction) => {
                        self.advance();
                        let name = self.expect_ident();
                        let func = self.finish_func_def(FuncKind::Async, name.clone());
                        match func.name.name.as_str() {
                            "__on_terminate__" => ThreadTemplateMember::OnTerminate(func),
                            _ => ThreadTemplateMember::MemberFunc(func),
                        }
                    }
                    _ => {
                        let span = self.current_span();
                        let got = self.describe_got();
                        self.errors.push(
                            ParseError::new(
                                format!("expected `handler` or `function` after `async`, but found {got}"),
                                span,
                            )
                            .primary_label("expected `handler` or `function` here")
                            .with_help("async members must be either `async handler` or `async function`"),
                        );
                        // skip one token for recovery
                        if !self.at_eof() { self.advance(); }
                        ThreadTemplateMember::MemberFunc(FuncDef {
                            kind: FuncKind::Async,
                            name: Ident::new("<error>", span),
                            params: vec![],
                            return_type: TypeExpr::Void,
                            body: Block { stmts: vec![], span },
                            span,
                        })
                    }
                }
            }
            Some(Token::KwFunction) => {
                self.advance();
                let name = self.expect_ident();
                let func = self.finish_func_def(FuncKind::Sync, name.clone());
                match func.name.name.as_str() {
                    "__on_enter__" => ThreadTemplateMember::OnEnter(func),
                    "__on_exit__"  => ThreadTemplateMember::OnExit(func),
                    _              => ThreadTemplateMember::MemberFunc(func),
                }
            }
            _ => {
                let span = self.current_span();
                let got = self.describe_got();
                self.errors.push(
                    ParseError::new(
                        format!("unexpected {got} in thread template body"),
                        span,
                    )
                    .primary_label("not valid here")
                    .with_help("a `$template` body may contain `expose`, `define`, handlers, and functions"),
                );
                if !self.at_eof() { self.advance(); }
                ThreadTemplateMember::MemberFunc(FuncDef {
                    kind: FuncKind::Sync,
                    name: Ident::new("<error>", span),
                    params: vec![],
                    return_type: TypeExpr::Void,
                    body: Block { stmts: vec![], span },
                    span,
                })
            }
        }
    }

    fn parse_expose_decl(&mut self) -> ExposeDecl {
        let name = self.expect_ident();
        self.expect(&Token::Colon);
        let ty = self.parse_type_expr();
        let initializer = if self.eat(&Token::Eq) { Some(self.parse_expr()) } else { None };
        let end = self.expect(&Token::Semicolon);
        ExposeDecl { name: name.clone(), ty, initializer, span: name.span.merge(end) }
    }

    fn parse_handler_def(&mut self) -> HandlerDef {
        let name = self.expect_ident();
        let params = self.parse_param_list();
        self.expect(&Token::Colon);
        let return_type = self.parse_type_expr();
        let body = self.parse_block();
        let span = name.span.merge(body.span);
        HandlerDef { name, params, return_type, body, span }
    }

    // ── Functions ─────────────────────────────────────────────────────────────

    fn finish_func_def(&mut self, kind: FuncKind, name: Ident) -> FuncDef {
        let params = self.parse_param_list();
        self.expect(&Token::Colon);
        let return_type = self.parse_type_expr();
        let body = self.parse_block();
        let span = name.span.merge(body.span);
        FuncDef { kind, name, params, return_type, body, span }
    }

    fn parse_param_list(&mut self) -> Vec<Param> {
        self.expect(&Token::ParenOpen);
        let mut params = Vec::new();
        while !matches!(self.peek(), Some(Token::ParenClose) | None) {
            params.push(self.parse_param());
            if !self.eat(&Token::Comma) { break; }
        }
        self.expect(&Token::ParenClose);
        params
    }

    fn parse_param_list_opt(&mut self) -> Vec<Param> {
        if matches!(self.peek(), Some(Token::ParenOpen)) {
            self.parse_param_list()
        } else {
            Vec::new()
        }
    }

    fn parse_param(&mut self) -> Param {
        let name = self.expect_ident();
        self.expect(&Token::Colon);
        let ty = self.parse_type_expr();
        let default = if self.eat(&Token::Eq) { Some(self.parse_expr()) } else { None };
        let span = name.span;
        Param { name, ty, default, span }
    }

    // ── Statements ────────────────────────────────────────────────────────────

    fn parse_block(&mut self) -> Block {
        let start = self.expect(&Token::BraceOpen);
        let mut stmts = Vec::new();
        while !matches!(self.peek(), Some(Token::BraceClose) | None) {
            stmts.push(self.parse_stmt());
        }
        let end = self.expect(&Token::BraceClose);
        Block { stmts, span: start.merge(end) }
    }

    fn parse_stmt(&mut self) -> Stmt {
        // Pre-binding thread spawn: `name = $Template(...) { ... }`
        if self.is_prebind_thread_spawn() {
            let name = self.expect_ident();
            self.expect(&Token::Eq);
            let mut ts = self.parse_thread_spawn_stmt();
            ts.handle_bind = HandleBind::Bind(name);
            return Stmt::ThreadSpawn(ts);
        }
        match self.peek().cloned() {
            Some(Token::KwLet) => Stmt::Let(self.parse_let_stmt()),
            Some(Token::KwIf) => Stmt::If(self.parse_if_stmt()),
            Some(Token::KwWhile) => Stmt::While(self.parse_while_stmt()),
            Some(Token::KwFor) => Stmt::For(self.parse_for_stmt()),
            Some(Token::KwReturn) => Stmt::Return(self.parse_return_stmt()),
            Some(Token::KwBreak) => {
                let span = self.advance().span;
                self.expect(&Token::Semicolon);
                Stmt::Break(span)
            }
            Some(Token::KwContinue) => {
                let span = self.advance().span;
                self.expect(&Token::Semicolon);
                Stmt::Continue(span)
            }
            Some(Token::KwExclusive) => Stmt::ExclusiveBlock(self.parse_exclusive_block()),
            // Thread spawn: $Name(...) { ... } or ${ ... } or $template { ... } { ... }
            Some(Token::Dollar) | Some(Token::KwDollarTemplate) | Some(Token::DollarBraceOpen) => {
                Stmt::ThreadSpawn(self.parse_thread_spawn_stmt())
            }
            // Scope block: @Name(...) { ... } or @template { ... } { ... }
            Some(Token::At) | Some(Token::KwAtTemplate) => {
                Stmt::ScopeBlock(self.parse_scope_block_stmt())
            }
            _ => self.parse_expr_or_assign_stmt(),
        }
    }

    /// Returns true when the next tokens are `Ident '=' ('$'|'${'|'$template')`.
    fn is_prebind_thread_spawn(&self) -> bool {
        matches!(self.tokens.get(self.pos).map(|s| &s.node), Some(Token::Ident(_)))
        && matches!(self.tokens.get(self.pos + 1).map(|s| &s.node), Some(Token::Eq))
        && matches!(
            self.tokens.get(self.pos + 2).map(|s| &s.node),
            Some(Token::Dollar) | Some(Token::DollarBraceOpen) | Some(Token::KwDollarTemplate)
        )
    }

    fn parse_let_stmt(&mut self) -> LetStmt {
        let start = self.expect(&Token::KwLet);
        let name = self.expect_ident();
        let ty = if self.eat(&Token::Colon) { Some(self.parse_type_expr()) } else { None };
        self.expect(&Token::Eq);
        let init = self.parse_expr();
        let end = self.expect(&Token::Semicolon);
        LetStmt { name, ty, init, span: start.merge(end) }
    }

    fn parse_if_stmt(&mut self) -> IfStmt {
        let start = self.expect(&Token::KwIf);
        self.expect(&Token::ParenOpen);
        let condition = self.parse_expr();
        self.expect(&Token::ParenClose);
        let then_block = self.parse_block();
        let else_branch = if self.eat(&Token::KwElse) {
            if matches!(self.peek(), Some(Token::KwIf)) {
                Some(ElseBranch::ElseIf(Box::new(self.parse_if_stmt())))
            } else {
                Some(ElseBranch::Else(self.parse_block()))
            }
        } else {
            None
        };
        let end = self.prev_span();
        IfStmt { condition, then_block, else_branch, span: start.merge(end) }
    }

    fn parse_while_stmt(&mut self) -> WhileStmt {
        let start = self.expect(&Token::KwWhile);
        self.expect(&Token::ParenOpen);
        let condition = self.parse_expr();
        self.expect(&Token::ParenClose);
        let body = self.parse_block();
        let span = start.merge(body.span);
        WhileStmt { condition, body, span }
    }

    fn parse_for_stmt(&mut self) -> ForStmt {
        let start = self.expect(&Token::KwFor);
        self.expect(&Token::ParenOpen);
        let init = if !matches!(self.peek(), Some(Token::Semicolon)) {
            Some(Box::new(self.parse_stmt()))
        } else {
            self.expect(&Token::Semicolon);
            None
        };
        let condition = if !matches!(self.peek(), Some(Token::Semicolon)) {
            Some(self.parse_expr())
        } else {
            None
        };
        self.expect(&Token::Semicolon);
        let update = if !matches!(self.peek(), Some(Token::ParenClose)) {
            Some(Box::new(self.parse_expr_or_assign_no_semi()))
        } else {
            None
        };
        self.expect(&Token::ParenClose);
        let body = self.parse_block();
        let span = start.merge(body.span);
        ForStmt { init, condition, update, body, span }
    }

    fn parse_return_stmt(&mut self) -> ReturnStmt {
        let start = self.expect(&Token::KwReturn);
        let value = if !matches!(self.peek(), Some(Token::Semicolon)) {
            Some(self.parse_expr())
        } else {
            None
        };
        let end = self.expect(&Token::Semicolon);
        ReturnStmt { value, span: start.merge(end) }
    }

    fn parse_exclusive_block(&mut self) -> ExclusiveBlockStmt {
        let start = self.expect(&Token::KwExclusive);
        let body = self.parse_block();
        let span = start.merge(body.span);
        ExclusiveBlockStmt { body, span }
    }

    fn parse_thread_spawn_stmt(&mut self) -> ThreadSpawnStmt {
        let start = self.current_span();

        let (template, args) = match self.peek().cloned() {
            Some(Token::KwDollarTemplate) => {
                let decl = self.parse_thread_template_decl();
                let args = self.parse_call_args();
                (ThreadTemplateRef::Anonymous(Box::new(decl)), args)
            }
            Some(Token::DollarBraceOpen) => {
                self.advance(); // consume `${`
                // shorthand: no args, body immediately follows (the `{` was consumed)
                // Re-create the block manually
                let block_start = self.prev_span();
                let mut stmts = Vec::new();
                while !matches!(self.peek(), Some(Token::BraceClose) | None) {
                    stmts.push(self.parse_stmt());
                }
                let end = self.expect(&Token::BraceClose);
                let body = Block { stmts, span: block_start.merge(end) };
                let handle = self.parse_handle_bind_opt();
                self.expect(&Token::Semicolon);
                return ThreadSpawnStmt {
                    template: ThreadTemplateRef::Shorthand,
                    args: vec![],
                    body,
                    handle_bind: handle,
                    span: start.merge(self.prev_span()),
                };
            }
            Some(Token::Dollar) => {
                self.advance();
                let name = self.expect_ident();
                let args = self.parse_call_args();
                (ThreadTemplateRef::Named(name), args)
            }
            _ => {
                let span = self.current_span();
                let got = self.describe_got();
                self.errors.push(
                    ParseError::new(format!("expected a thread spawn, but found {got}"), span)
                        .primary_label("expected `$template`, `$Name(...)` or `${ ... }` here"),
                );
                (ThreadTemplateRef::Shorthand, vec![])
            }
        };

        let body = self.parse_block();
        let handle = self.parse_handle_bind_opt();
        let end = self.expect(&Token::Semicolon);
        ThreadSpawnStmt { template, args, body, handle_bind: handle, span: start.merge(end) }
    }

    fn parse_handle_bind_opt(&mut self) -> HandleBind {
        if self.eat(&Token::ColonEq) {
            HandleBind::Bind(self.expect_ident())
        } else {
            HandleBind::Discard
        }
    }

    fn parse_scope_block_stmt(&mut self) -> ScopeBlockStmt {
        let start = self.current_span();
        let (template, args) = match self.peek().cloned() {
            Some(Token::KwAtTemplate) => {
                let decl = self.parse_scope_template_decl();
                let args = self.parse_call_args();
                (ScopeTemplateRef::Anonymous(Box::new(decl)), args)
            }
            Some(Token::At) => {
                self.advance();
                let name = self.expect_ident();
                let args = self.parse_call_args();
                (ScopeTemplateRef::Named(name), args)
            }
            _ => {
                let span = self.current_span();
                let got = self.describe_got();
                self.errors.push(
                    ParseError::new(format!("expected a scope block, but found {got}"), span)
                        .primary_label("expected `@template` or `@Name(...)` here"),
                );
                (ScopeTemplateRef::Named(Ident::new("<error>", span)), vec![])
            }
        };
        let body = self.parse_block();
        let span = start.merge(body.span);
        ScopeBlockStmt { template, args, body, span }
    }

    fn parse_expr_or_assign_stmt(&mut self) -> Stmt {
        let expr = self.parse_expr();
        if self.eat(&Token::Eq) {
            let value = self.parse_expr();
            let end = self.expect(&Token::Semicolon);
            let target = expr_to_assign_target(expr, &mut self.errors);
            let span = target.span().merge(end);
            return Stmt::Assign(AssignStmt { target, value, span });
        }
        let end = self.expect(&Token::Semicolon);
        let span = expr.span().merge(end);
        Stmt::Expr(ExprStmt { expr, span })
    }

    /// Like parse_expr_or_assign_stmt but does NOT consume a trailing semicolon.
    /// Used for the update clause of a for loop.
    fn parse_expr_or_assign_no_semi(&mut self) -> Stmt {
        let expr = self.parse_expr();
        if self.eat(&Token::Eq) {
            let value = self.parse_expr();
            let target = expr_to_assign_target(expr, &mut self.errors);
            let span = target.span().merge(value.span());
            return Stmt::Assign(AssignStmt { target, value, span });
        }
        let span = expr.span();
        Stmt::Expr(ExprStmt { expr, span })
    }

    fn parse_call_args(&mut self) -> Vec<Expr> {
        self.expect(&Token::ParenOpen);
        let mut args = Vec::new();
        while !matches!(self.peek(), Some(Token::ParenClose) | None) {
            args.push(self.parse_expr());
            if !self.eat(&Token::Comma) { break; }
        }
        self.expect(&Token::ParenClose);
        args
    }

    // ── Expressions (Pratt parser) ────────────────────────────────────────────

    fn parse_expr(&mut self) -> Expr {
        self.parse_pratt(0)
    }

    fn parse_pratt(&mut self, min_bp: u8) -> Expr {
        let mut lhs = self.parse_unary();

        while let Some(op) = self.peek_binop() {
            let (lbp, rbp) = infix_binding_power(&op);
            if lbp < min_bp { break; }
            self.advance();
            let rhs = self.parse_pratt(rbp);
            let span = lhs.span().merge(rhs.span());
            lhs = Expr::BinOp(Box::new(BinOpExpr { op, left: lhs, right: rhs, span }));
        }

        lhs
    }

    fn peek_binop(&self) -> Option<BinOp> {
        match self.peek()? {
            Token::Plus    => Some(BinOp::Add),
            Token::Minus   => Some(BinOp::Sub),
            Token::Star    => Some(BinOp::Mul),
            Token::Slash   => Some(BinOp::Div),
            Token::Percent => Some(BinOp::Rem),
            Token::EqEq    => Some(BinOp::Eq),
            Token::BangEq  => Some(BinOp::Ne),
            Token::Lt if !self.in_type_ctx => Some(BinOp::Lt),
            Token::LtEq    => Some(BinOp::Le),
            Token::Gt if !self.in_type_ctx => Some(BinOp::Gt),
            Token::GtEq    => Some(BinOp::Ge),
            Token::AmpAmp  => Some(BinOp::And),
            Token::PipePipe => Some(BinOp::Or),
            _ => None,
        }
    }

    fn parse_unary(&mut self) -> Expr {
        let span = self.current_span();
        match self.peek().cloned() {
            Some(Token::Bang) => {
                self.advance();
                let operand = self.parse_unary();
                let end = operand.span();
                Expr::UnaryOp(Box::new(UnaryOpExpr { op: UnaryOp::Not, operand, span: span.merge(end) }))
            }
            Some(Token::Minus) => {
                self.advance();
                let operand = self.parse_unary();
                let end = operand.span();
                Expr::UnaryOp(Box::new(UnaryOpExpr { op: UnaryOp::Neg, operand, span: span.merge(end) }))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Expr {
        let mut expr = self.parse_primary();
        loop {
            match self.peek().cloned() {
                Some(Token::Dot) => {
                    self.advance();
                    let field = self.expect_ident();
                    if matches!(self.peek(), Some(Token::ParenOpen)) {
                        let args = self.parse_call_args();
                        let span = expr.span().merge(self.prev_span());
                        expr = Expr::MethodCall(Box::new(MethodCallExpr { receiver: expr, method: field, args, span }));
                    } else {
                        let span = expr.span().merge(field.span);
                        expr = Expr::FieldAccess(Box::new(FieldAccessExpr { object: expr, field, span }));
                    }
                }
                Some(Token::BracketOpen) => {
                    self.advance();
                    let index = self.parse_expr();
                    let end = self.expect(&Token::BracketClose);
                    let span = expr.span().merge(end);
                    expr = Expr::Index(Box::new(IndexExpr { object: expr, index, span }));
                }
                _ => break,
            }
        }
        expr
    }

    fn parse_primary(&mut self) -> Expr {
        let span = self.current_span();
        match self.peek().cloned() {
            Some(Token::LitTrue)  => { self.advance(); Expr::Lit(Literal { kind: LitKind::Bool(true), span }) }
            Some(Token::LitFalse) => { self.advance(); Expr::Lit(Literal { kind: LitKind::Bool(false), span }) }
            Some(Token::LitInt(n)) => { self.advance(); Expr::Lit(Literal { kind: LitKind::Int(n), span }) }
            Some(Token::LitDouble(f)) => { self.advance(); Expr::Lit(Literal { kind: LitKind::Double(f), span }) }
            Some(Token::LitString(s)) => { self.advance(); Expr::Lit(Literal { kind: LitKind::String(s), span }) }
            Some(Token::LitChar(c)) => { self.advance(); Expr::Lit(Literal { kind: LitKind::Char(c), span }) }
            Some(Token::KwPanic) => {
                self.advance();
                self.expect(&Token::ParenOpen);
                let message = self.parse_expr();
                let end = self.expect(&Token::ParenClose);
                Expr::Panic(Box::new(PanicExpr { message, span: span.merge(end) }))
            }
            Some(Token::KwAssert) => {
                self.advance();
                self.expect(&Token::ParenOpen);
                let condition = self.parse_expr();
                let message = if self.eat(&Token::Comma) { Some(self.parse_expr()) } else { None };
                let end = self.expect(&Token::ParenClose);
                Expr::Assert(Box::new(AssertExpr { condition, message, span: span.merge(end) }))
            }
            Some(Token::KwAwait) => {
                self.advance();
                let inner = self.parse_unary();
                let end = inner.span();
                Expr::Await(Box::new(AwaitExpr { expr: inner, span: span.merge(end) }))
            }
            Some(Token::KwTry) => {
                self.advance();
                let inner = self.parse_unary();
                let end = inner.span();
                Expr::Try(Box::new(TryExpr { expr: inner, span: span.merge(end) }))
            }
            Some(Token::ParenOpen) => {
                self.advance();
                let inner = self.parse_expr();
                self.expect(&Token::ParenClose);
                inner
            }
            // Identifiers: may be plain ident, call, or type constructor (Ok, Err, Some, List<T>, ...)
            Some(Token::Ident(name)) => {
                self.advance();
                self.parse_ident_or_call(name, span)
            }
            // Type-keyword-named constructors: bool, int, String, etc.
            Some(tok) if is_type_keyword(&tok) => {
                let name = type_keyword_name(&tok).to_string();
                self.advance();
                self.parse_ident_or_call(name, span)
            }
            _ => {
                let got = self.describe_got();
                self.errors.push(
                    ParseError::new(format!("unexpected {got} while parsing an expression"), span)
                        .primary_label("expected a value, identifier, or `(`"),
                );
                if !self.at_eof() { self.advance(); }
                Expr::Ident(Ident::new("<error>", span))
            }
        }
    }

    fn parse_ident_or_call(&mut self, name: String, span: Span) -> Expr {
        // check for generic type constructor: Name<T,...>(args)
        if matches!(self.peek(), Some(Token::Lt)) && self.looks_like_type_args() {
            let type_args = self.parse_type_arg_list();
            let args = self.parse_call_args();
            let end = self.prev_span();
            return Expr::TypeCtor(Box::new(TypeCtorExpr { name, type_args, args, span: span.merge(end) }));
        }
        // plain call: Name(args)
        if matches!(self.peek(), Some(Token::ParenOpen)) {
            let args = self.parse_call_args();
            let end = self.prev_span();
            // For Ok/Err/Some/None, use TypeCtor with no explicit type args
            match name.as_str() {
                "Ok" | "Err" | "Some" | "List" | "locked" | "Queue" => {
                    return Expr::TypeCtor(Box::new(TypeCtorExpr { name, type_args: vec![], args, span: span.merge(end) }));
                }
                "None" => {
                    return Expr::TypeCtor(Box::new(TypeCtorExpr { name, type_args: vec![], args: vec![], span: span.merge(end) }));
                }
                _ => {
                    let ident = Ident::new(name, span);
                    return Expr::Call(Box::new(CallExpr { callee: Expr::Ident(ident), args, span: span.merge(end) }));
                }
            }
        }
        // Special singleton: None
        if name == "None" {
            return Expr::TypeCtor(Box::new(TypeCtorExpr { name, type_args: vec![], args: vec![], span }));
        }
        Expr::Ident(Ident::new(name, span))
    }

    /// Heuristic: after `<`, is this a type arg list?
    /// Requires the token after `<` to look like a type name AND be followed
    /// by `>`, `,`, or `<` (for nested generics) — not an operator or semicolon.
    fn looks_like_type_args(&self) -> bool {
        let is_type_start = matches!(
            self.tokens.get(self.pos + 1).map(|s| &s.node),
            Some(Token::Ident(_)) | Some(Token::KwBool) | Some(Token::KwInt)
            | Some(Token::KwDouble) | Some(Token::KwChar) | Some(Token::KwString)
            | Some(Token::KwVoid) | Some(Token::KwNever)
        );
        if !is_type_start { return false; }
        // Check that the token after the type name is `>`, `,`, or `<` (nested generic)
        matches!(
            self.tokens.get(self.pos + 2).map(|s| &s.node),
            Some(Token::Gt) | Some(Token::Comma) | Some(Token::Lt)
        )
    }

    fn parse_type_arg_list(&mut self) -> Vec<TypeExpr> {
        self.expect(&Token::Lt);
        let old = self.in_type_ctx;
        self.in_type_ctx = true;
        let mut args = Vec::new();
        while !matches!(self.peek(), Some(Token::Gt) | None) {
            args.push(self.parse_type_expr());
            if !self.eat(&Token::Comma) { break; }
        }
        self.expect(&Token::Gt);
        self.in_type_ctx = old;
        args
    }

    // ── Types ─────────────────────────────────────────────────────────────────

    fn parse_type_expr(&mut self) -> TypeExpr {
        let old = self.in_type_ctx;
        self.in_type_ctx = true;
        let result = self.parse_type_expr_inner();
        self.in_type_ctx = old;
        result
    }

    fn parse_type_expr_inner(&mut self) -> TypeExpr {
        let span = self.current_span();
        match self.peek().cloned() {
            Some(Token::KwVoid) => { self.advance(); TypeExpr::Void }
            Some(Token::KwNever) => { self.advance(); TypeExpr::Never }
            Some(Token::Ident(name)) => {
                let ident = Ident::new(name, span);
                self.advance();
                let type_args = if matches!(self.peek(), Some(Token::Lt)) {
                    self.parse_type_arg_list()
                } else {
                    vec![]
                };
                TypeExpr::Named(ident, type_args)
            }
            Some(tok) if is_type_keyword(&tok) => {
                let name = type_keyword_name(&tok);
                let ident = Ident::new(name, span);
                self.advance();
                let type_args = if matches!(self.peek(), Some(Token::Lt)) {
                    self.parse_type_arg_list()
                } else {
                    vec![]
                };
                TypeExpr::Named(ident, type_args)
            }
            other => {
                self.errors.push(ParseError::new(
                    format!("expected type, got {:?}", other), span,
                ));
                TypeExpr::Named(Ident::new("<error>", span), vec![])
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn infix_binding_power(op: &BinOp) -> (u8, u8) {
    match op {
        BinOp::Or           => (1, 2),
        BinOp::And          => (3, 4),
        BinOp::Eq | BinOp::Ne => (5, 6),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => (7, 8),
        BinOp::Add | BinOp::Sub => (9, 10),
        BinOp::Mul | BinOp::Div | BinOp::Rem => (11, 12),
    }
}

fn is_type_keyword(tok: &Token) -> bool {
    matches!(tok, Token::KwBool | Token::KwInt | Token::KwDouble
        | Token::KwChar | Token::KwString | Token::KwVoid | Token::KwNever)
}

fn type_keyword_name(tok: &Token) -> &'static str {
    match tok {
        Token::KwBool   => "bool",
        Token::KwInt    => "int",
        Token::KwDouble => "double",
        Token::KwChar   => "char",
        Token::KwString => "String",
        Token::KwVoid   => "void",
        Token::KwNever  => "never",
        _ => "<error>",
    }
}

fn expr_to_assign_target(expr: Expr, errors: &mut Vec<ParseError>) -> AssignTarget {
    match expr {
        Expr::Ident(i) => AssignTarget::Ident(i),
        Expr::FieldAccess(f) => AssignTarget::Field(Box::new(f.object), f.field),
        Expr::Index(i) => AssignTarget::Index(Box::new(i.object), Box::new(i.index)),
        other => {
            let span = other.span();
            errors.push(
                ParseError::new("invalid assignment target", span)
                    .primary_label("cannot assign to this expression")
                    .with_help("the left-hand side of `=` must be a variable, field, or index"),
            );
            AssignTarget::Ident(Ident::new("<error>", span))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_lexer::lex;

    fn parse_src(src: &str) -> Program {
        let (prog, errors) = crate::parse(lex(src));
        if !errors.is_empty() {
            panic!("parse errors: {:?}", errors);
        }
        prog
    }

    #[test]
    fn let_stmt() {
        let prog = parse_src("let x: int = 42;");
        assert!(matches!(
            prog.items[0],
            TopLevelItem::Statement(Stmt::Let(_))
        ));
    }

    #[test]
    fn scope_template() {
        let prog = parse_src("@template Foo() { function __on_enter__(): void {} }");
        assert!(matches!(prog.items[0], TopLevelItem::ScopeTemplateDecl(_)));
    }

    #[test]
    fn thread_template_with_expose_and_handler() {
        let src = r#"
            $template Worker(n: int) {
                expose state: int = 0;
                async handler update(delta: int): void {}
                async function __on_terminate__(): void {}
            }
        "#;
        let prog = parse_src(src);
        assert!(matches!(prog.items[0], TopLevelItem::ThreadTemplateDecl(_)));
        if let TopLevelItem::ThreadTemplateDecl(ref td) = prog.items[0] {
            let has_terminate = td.members.iter().any(|m| matches!(m, ThreadTemplateMember::OnTerminate(_)));
            let has_expose    = td.members.iter().any(|m| matches!(m, ThreadTemplateMember::Expose(_)));
            let has_handler   = td.members.iter().any(|m| matches!(m, ThreadTemplateMember::Handler(_)));
            assert!(has_terminate, "missing __on_terminate__");
            assert!(has_expose, "missing expose");
            assert!(has_handler, "missing handler");
        }
    }

    #[test]
    fn exclusive_block() {
        let prog = parse_src("#exclusive { let x = 1; }");
        assert!(matches!(
            prog.items[0],
            TopLevelItem::Statement(Stmt::ExclusiveBlock(_))
        ));
    }

    #[test]
    fn pratt_precedence() {
        // 1 + 2 * 3 should parse as 1 + (2 * 3)
        let prog = parse_src("1 + 2 * 3;");
        if let TopLevelItem::Statement(Stmt::Expr(es)) = &prog.items[0] {
            if let Expr::BinOp(b) = &es.expr {
                assert_eq!(b.op, BinOp::Add);
                assert!(matches!(&b.right, Expr::BinOp(r) if r.op == BinOp::Mul));
            } else {
                panic!("expected BinOp");
            }
        }
    }
}
