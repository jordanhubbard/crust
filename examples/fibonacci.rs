// fibonacci.rs — runs in crust without modification
fn fib(n: i32) -> i32 {
    match n {
        0 => 0,
        1 => 1,
        _ => fib(n - 1) + fib(n - 2),
    }
}

fn main() {
    for i in 0..10 {
        println!("fib({}) = {}", i, fib(i));
    }
}
