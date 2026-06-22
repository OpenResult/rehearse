#![allow(dead_code)]

use rehearse::{BoxFuture, Impact, Input, Operation, OperationMetadata};
use std::fmt;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestError {
    Boom(&'static str),
}

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boom(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for TestError {}

#[derive(Debug, Clone, Default)]
pub struct TestContext {
    calls: Arc<Mutex<Vec<&'static str>>>,
}

impl TestContext {
    pub fn record(&self, name: &'static str) {
        self.calls.lock().expect("calls mutex poisoned").push(name);
    }

    pub fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().expect("calls mutex poisoned").clone()
    }
}

pub fn metadata(name: &'static str, impact: Impact) -> OperationMetadata {
    OperationMetadata::new(name, impact)
}

pub fn op0<T>(name: &'static str, impact: Impact, output: T) -> Operation<TestContext, T, TestError>
where
    T: Clone + Send + Sync + 'static,
{
    Operation::new(
        metadata(name, impact),
        (),
        move |context: &TestContext, ()| -> BoxFuture<'_, Result<T, TestError>> {
            let context = context.clone();
            let output = output.clone();
            Box::pin(async move {
                context.record(name);
                Ok(output)
            })
        },
    )
}

pub fn fail0<T>(
    name: &'static str,
    impact: Impact,
    message: &'static str,
) -> Operation<TestContext, T, TestError>
where
    T: Clone + Send + Sync + 'static,
{
    Operation::new(
        metadata(name, impact),
        (),
        move |context: &TestContext, ()| -> BoxFuture<'_, Result<T, TestError>> {
            let context = context.clone();
            Box::pin(async move {
                context.record(name);
                Err(TestError::Boom(message))
            })
        },
    )
}

pub fn panic0<T>(name: &'static str, impact: Impact) -> Operation<TestContext, T, TestError>
where
    T: Clone + Send + Sync + 'static,
{
    Operation::new(
        metadata(name, impact),
        (),
        move |context: &TestContext, ()| -> BoxFuture<'_, Result<T, TestError>> {
            let context = context.clone();
            Box::pin(async move {
                context.record(name);
                panic!("{name} should not have been invoked");
            })
        },
    )
}

pub fn op1<A, T, F>(
    name: &'static str,
    impact: Impact,
    input: Input<A>,
    f: F,
) -> Operation<TestContext, T, TestError>
where
    A: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    F: Fn(A) -> T + Send + Sync + 'static,
{
    let f = Arc::new(f);
    Operation::new(
        metadata(name, impact),
        input,
        move |context: &TestContext, input: A| -> BoxFuture<'_, Result<T, TestError>> {
            let context = context.clone();
            let f = Arc::clone(&f);
            Box::pin(async move {
                context.record(name);
                Ok(f(input))
            })
        },
    )
}

pub fn fail1<A, T>(
    name: &'static str,
    impact: Impact,
    input: Input<A>,
    message: &'static str,
) -> Operation<TestContext, T, TestError>
where
    A: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    Operation::new(
        metadata(name, impact),
        input,
        move |context: &TestContext, _input: A| -> BoxFuture<'_, Result<T, TestError>> {
            let context = context.clone();
            Box::pin(async move {
                context.record(name);
                Err(TestError::Boom(message))
            })
        },
    )
}

pub fn panic1<A, T>(
    name: &'static str,
    impact: Impact,
    input: Input<A>,
) -> Operation<TestContext, T, TestError>
where
    A: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    Operation::new(
        metadata(name, impact),
        input,
        move |context: &TestContext, _input: A| -> BoxFuture<'_, Result<T, TestError>> {
            let context = context.clone();
            Box::pin(async move {
                context.record(name);
                panic!("{name} should not have been invoked");
            })
        },
    )
}

pub fn op2<A, B, T, F>(
    name: &'static str,
    impact: Impact,
    inputs: (Input<A>, Input<B>),
    f: F,
) -> Operation<TestContext, T, TestError>
where
    A: Clone + Send + Sync + 'static,
    B: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    F: Fn(A, B) -> T + Send + Sync + 'static,
{
    let f = Arc::new(f);
    Operation::new(
        metadata(name, impact),
        inputs,
        move |context: &TestContext, (a, b): (A, B)| -> BoxFuture<'_, Result<T, TestError>> {
            let context = context.clone();
            let f = Arc::clone(&f);
            Box::pin(async move {
                context.record(name);
                Ok(f(a, b))
            })
        },
    )
}

pub fn op3<A, B, C, T, F>(
    name: &'static str,
    impact: Impact,
    inputs: (Input<A>, Input<B>, Input<C>),
    f: F,
) -> Operation<TestContext, T, TestError>
where
    A: Clone + Send + Sync + 'static,
    B: Clone + Send + Sync + 'static,
    C: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    F: Fn(A, B, C) -> T + Send + Sync + 'static,
{
    let f = Arc::new(f);
    Operation::new(
        metadata(name, impact),
        inputs,
        move |context: &TestContext, (a, b, c): (A, B, C)| -> BoxFuture<'_, Result<T, TestError>> {
            let context = context.clone();
            let f = Arc::clone(&f);
            Box::pin(async move {
                context.record(name);
                Ok(f(a, b, c))
            })
        },
    )
}
