def greet(name: str) -> str:
    return f"Hello, {name}!"

names = ["world", "Rust", "Python"]
for name in names:
    print(greet(name))
