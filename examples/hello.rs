// hello.rs — the simplest crust program
fn main() {
    let name = "world";
    println!("Hello, {}!", name);

    let numbers = vec![1, 2, 3, 4, 5];
    let sum: i32 = numbers.iter().sum();
    println!("Sum of {:?} = {}", numbers, sum);
}
