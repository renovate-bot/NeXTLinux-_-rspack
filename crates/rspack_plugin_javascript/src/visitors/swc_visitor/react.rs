use crate::ast::parse;
use rspack_core::ModuleType;
use rspack_core::ReactOptions;
use std::{path::Path, sync::Arc};
use sugar_path::PathSugar;
use swc_common::{comments::SingleThreadedComments, Mark, SourceMap};
use swc_ecma_ast::{CallExpr, Callee, Expr, Module, Program};
use swc_ecma_transforms::react::{react as swc_react, Options};
use swc_ecma_visit::{Fold, Visit, VisitWith};

pub fn react<'a>(
  top_level_mark: Mark,
  comments: Option<&'a SingleThreadedComments>,
  cm: &Arc<SourceMap>,
  options: &ReactOptions,
) -> impl Fold + 'a {
  swc_react(
    cm.clone(),
    comments,
    Options {
      refresh: options.development.and_then(|dev| {
        if dev {
          Some(swc_ecma_transforms::react::RefreshOptions::default())
        } else {
          None
        }
      }),
      runtime: options.runtime,
      import_source: options.import_source.clone(),
      pragma: options.pragma.clone(),
      pragma_frag: options.pragma_frag.clone(),
      throw_if_namespace: options.throw_if_namespace,
      development: options.development,
      use_builtins: options.use_builtins,
      use_spread: options.use_spread,
      ..Default::default()
    },
    top_level_mark,
  )
}

pub fn fold_react_refresh(context: &str, uri: &str) -> impl Fold {
  ReactHmrFolder {
    id: Path::new(uri)
      .relative(Path::new(context))
      .to_string_lossy()
      .to_string(),
  }
}

pub struct FoundReactRefreshVisitor {
  pub is_refresh_boundary: bool,
}

impl Visit for FoundReactRefreshVisitor {
  fn visit_call_expr(&mut self, call_expr: &CallExpr) {
    if let Callee::Expr(expr) = &call_expr.callee {
      if let Expr::Ident(ident) = &**expr {
        if "$RefreshReg$".eq(&ident.sym) {
          self.is_refresh_boundary = true;
        }
      }
    }
  }
}

static HMR_HEADER: &str = r#"var RefreshRuntime = __rspack_require__('/react-refresh');
var prevRefreshReg;
var prevRefreshSig;
prevRefreshReg = globalThis.$RefreshReg$;
prevRefreshSig = globalThis.$RefreshSig$;
globalThis.$RefreshReg$ = (type, id) => {
  RefreshRuntime.register(type, "__SOURCE__" + "_" + id);
};
globalThis.$RefreshSig$ = RefreshRuntime.createSignatureFunctionForTransform;"#;

static HMR_FOOTER: &str = r#"var RefreshRuntime = __rspack_require__('/react-refresh');
globalThis.$RefreshReg$ = prevRefreshReg;
globalThis.$RefreshSig$ = prevRefreshSig;
module.hot.accept();
RefreshRuntime.queueUpdate();
"#;

pub struct ReactHmrFolder {
  pub id: String,
}

impl Fold for ReactHmrFolder {
  fn fold_module(&mut self, mut module: Module) -> Module {
    let mut f = FoundReactRefreshVisitor {
      is_refresh_boundary: false,
    };

    module.visit_with(&mut f);
    if !f.is_refresh_boundary {
      return module;
    }
    // TODO: cache the ast
    let hmr_header_ast = parse(
      HMR_HEADER.replace("__SOURCE__", self.id.as_str()),
      "",
      &ModuleType::Js,
    )
    .unwrap()
    .take_inner();

    // TODO: cache the ast
    let hmr_footer_ast = parse(HMR_FOOTER.to_string(), "", &ModuleType::Js)
      .unwrap()
      .take_inner();

    let mut body = vec![];
    body.append(&mut match hmr_header_ast {
      Program::Module(m) => m.body,
      _ => vec![],
    });
    body.append(&mut module.body);
    body.append(&mut match hmr_footer_ast {
      Program::Module(m) => m.body,
      _ => vec![],
    });

    Module { body, ..module }
  }
}