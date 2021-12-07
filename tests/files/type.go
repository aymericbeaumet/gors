package main

type my_int = int

type ()

type (
	Polar    = polar
)

type (
	Point struct{ x, y float64 }
	polar Point
)

type NewMutex Mutex
