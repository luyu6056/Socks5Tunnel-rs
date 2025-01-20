use crate::http1::Method;
use crate::http1::Request;

use crate::response::Response;
use crate::Result;
use futures_util::future::BoxFuture;
use matchit::Router as m_Router;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;


type HandlerFn<T> = fn(Request, T) -> Result<Response>;
type AsyncHandlerFn<T> = Rc<dyn Fn(Request, T) -> BoxFuture<'static, Result<Response>> + Send>;

pub(crate) enum Handler<T> {
    Async(AsyncHandlerFn<T>),
    Sync(HandlerFn<T>),
}
unsafe impl<T> Send for Handler<T> {}
unsafe impl<T> Sync for Handler<T> {}
impl<T> Clone for Handler<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Async(rc) => Self::Async((*rc).clone()),
            Self::Sync(func) => Self::Sync(*func),
        }
    }
}

impl<T> Handler<T> {
    pub(crate) async fn call(&self, req: Request, ctx: T) -> Result<Response> {
        match self {
            Handler::Async(func) => (func.as_ref())(req, ctx).await,
            Handler::Sync(func) => (func)(req, ctx),
        }
    }
}
#[derive(Clone)]
//cloudflare风格的router
pub struct Router<T> {
    /// 内部路由器，用于处理具体的路由逻辑
    inner: m_Router<Box<RouterEnum<T>>>,
    /// 在handler之前执行的中间件层
    before_handler_layer: Vec<BeforeHandlerLayerFn<T>>,
    /// 在handler之后执行的中间件层
    after_handler_layer: Vec<AfterHandlerLayerFn>,
}
#[derive(Clone)]
pub struct BeforeHandlerLayerFn<T> {
    inner: Rc<dyn Fn(Request, T) -> BoxFuture<'static, Result<LayerResult<T>>> + Send>,
}
unsafe impl<T> Send for BeforeHandlerLayerFn<T> {}
unsafe impl<T> Sync for BeforeHandlerLayerFn<T> {}
#[derive(Clone)]
struct AfterHandlerLayerFn {
    inner: Rc<dyn Fn(Response) -> BoxFuture<'static, Result<Response>> + Send>,
}
unsafe impl Send for AfterHandlerLayerFn {}
unsafe impl Sync for AfterHandlerLayerFn {}
// type BeforeHandlerLayerFn<T> =
// Rc<dyn Fn(Request, T) -> BoxFuture<'static, Result<LayerResult<T>>> + Send>;
// type AfterHandlerLayerFn = Rc<dyn Fn(Response) -> BoxFuture<'static, Result<Response>> + Send>;
impl<T> BeforeHandlerLayerFn<T> {
    pub(crate) async fn call(&self, req: Request, ctx: T) -> Result<LayerResult<T>> {
        (self.inner.as_ref())(req, ctx).await
    }
}
impl AfterHandlerLayerFn {
    pub(crate) async fn call(&self, resp: Response) -> Result<Response> {
        (self.inner.as_ref())(resp).await
    }
}
#[derive(Clone)]
pub(crate) enum RouterEnum<T> {
    GetRouter(Handler<T>),
    PostRouter(Handler<T>),
    NestRouter(Router<T>),
}

unsafe impl<T> Send for Router<T> {}
unsafe impl<T> Sync for Router<T> {}

impl<T: Send + 'static> Router<T> {
    pub fn new() -> Self {
        Self {
            inner: m_Router::new(),
            before_handler_layer: vec![],
            after_handler_layer: vec![],
        }
    }
    pub fn layer(mut self, layer: impl Layer<T> + 'static) -> Self {
        let l = layer.clone();
        let h: BeforeHandlerLayerFn<T> = BeforeHandlerLayerFn {
            inner: Rc::new(move |req, ctx| {
                let l = l.clone();
                Box::pin(l.before_handler_layer(req, ctx))
            }),
        };
        self.before_handler_layer.push(h);
        let h: AfterHandlerLayerFn = AfterHandlerLayerFn {
            inner: Rc::new(move |resp| {
                let l = layer.clone();
                Box::pin(l.after_handler_layer(resp))
            }),
        };
        self.after_handler_layer.push(h);
        self
    }
    pub fn get_async<F>(mut self, pattern: &str, func: fn(Request, T) -> F) -> Self
    where
        F: Future<Output = Result<Response>> + 'static + Send,
    {
        self.add_handler(
            pattern,
            Handler::Async(Rc::new(move |req, ctx| Box::pin(func(req, ctx)))),
            vec![Method::Get],
        );
        self
    }

    pub fn post_async<F>(mut self, pattern: &str, func: fn(Request, T) -> F) -> Self
    where
        F: Future<Output = Result<Response>> + 'static + Send,
    {
        self.add_handler(
            pattern,
            Handler::Async(Rc::new(move |req, ctx| Box::pin(func(req, ctx)))),
            vec![Method::Post],
        );
        self
    }
    pub fn any_async<F>(mut self, pattern: &str, func: fn(Request, T) -> F) -> Self
    where
        F: Future<Output = Result<Response>> + 'static + Send,
    {
        self.add_handler(pattern, Handler::Async(Rc::new(move |req, ctx| Box::pin(func(req, ctx)))), vec![Method::Get, Method::Post]);
        self
    }
    fn add_handler(&mut self, pattern: &str, func: Handler<T>, methods: Vec<Method>) {
        for method in methods {
            let f = match method {
                Method::Get => RouterEnum::GetRouter(func.clone()),
                Method::Post => RouterEnum::PostRouter(func.clone()),
                Method::Put => {
                    panic!("未处理")
                }
                Method::Patch => {
                    panic!("未处理")
                }
                Method::Delete => {
                    panic!("未处理")
                }
                Method::Options => {
                    panic!("未处理")
                }
                Method::Connect => {
                    panic!("未处理")
                }
                Method::Trace => {
                    panic!("未处理")
                }
                Method::Head => {
                    panic!("未处理")
                }
                _ => {
                    panic!("未处理 _")
                }
            };

            self.inner.insert(pattern, Box::new(f)).unwrap_or_else(|e| {
                panic!(
                    "failed to register {:?} route for {} pattern: {}",
                    method, pattern, e
                )
            });
        }
    }
    //例如传入/index，则为/index/*匹配一个子路由
    pub fn nest(mut self, pattern: &str, other: Self) -> Self {
        self.inner
            .insert(
                pattern.to_owned() + "/{*nest_param}",
                Box::new(RouterEnum::NestRouter(other)),
            )
            .unwrap_or_else(|e| {
                panic!(
                    "failed to register nest route for {} pattern: {}",
                    pattern, e
                )
            });
        self
    }

    pub async fn run(&mut self,  req: Request,  ctx: T) -> Result<Response> {
        match self.do_run(req.path().to_string(), req, ctx).await{
            Ok(resp) => {Ok(resp)}
            Err(err) => {Response::error(err.to_string(), 500)}
        }
    }
    async fn do_run(&self, path: String, mut req: Request, mut ctx: T) -> Result<Response> {
        let router = &self.inner;
        if let Ok(m) = router.at(path.as_str()) {
            let h = match m.value.as_ref() {
                RouterEnum::GetRouter(h) => h,
                RouterEnum::PostRouter(h) => h,
                RouterEnum::NestRouter(r) => {
                    if let Some(m_path) = m.params.get("nest_param") {
                        let new_path = "/".to_owned() + m_path;
                        let fut = async {
                            // 递归调用的逻辑
                            r.do_run(new_path, req, ctx).await
                        };
                        let pinned_fut = Box::pin(fut);
                        return Ok(pinned_fut.await?);
                    }
                    return Response::error("not found", 404);
                }
            };
            //执行前中间件
            for layer in &self.before_handler_layer {
                match layer.call(req, ctx).await? {
                    LayerResult::Next((next_req, next_ctx)) => {
                        req = next_req;
                        ctx = next_ctx
                    }
                    LayerResult::Response(resp) => return Ok(resp),
                }
            }
            let mut resp = h.call(req.clone(), ctx).await?;
            //执行后中间件
            for layer in &self.after_handler_layer {
                resp = layer.call(resp).await?;
            }
            return Ok(resp);
        }

        return Response::error("not found", 404);
    }
}
pub enum LayerResult<T> {
    Next((Request, T)),
    Response(Response),
}
pub trait Layer<T: Send + 'static>: Send + Sync + Clone {
    // 其他不使用 impl Trait 的方法
    fn before_handler_layer(
        self,
        req: Request,
        ctx: T,
    ) -> Pin<Box<dyn Future<Output = Result<LayerResult<T>>> + Send>> {
        Box::pin(async move { Ok((req, ctx).into()) })
    }
    fn after_handler_layer(
        self,
        resp: Response,
    ) -> Pin<Box<dyn Future<Output = Result<Response>> + Send>> {
        Box::pin(async move { Ok(resp) })
    }
}

impl<T> From<Response> for LayerResult<T> {
    fn from(resp: Response) -> Self {
        LayerResult::Response(resp)
    }
}
impl<T> From<(Request, T)> for LayerResult<T> {
    fn from(req: (Request, T)) -> Self {
        LayerResult::Next(req)
    }
}
