class Point:
    def __init__(self, x: float, y: float):
        self.x = x
        self.y = y

    def distance(self, other: 'Point') -> float:
        return ((self.x - other.x)**2 + (self.y - other.y)**2)**0.5

    def __repr__(self):
        return f"Point({self.x}, {self.y})"

a = Point(0.0, 0.0)
b = Point(3.0, 4.0)
print(f"Distance from {a} to {b}: {a.distance(b)}")
