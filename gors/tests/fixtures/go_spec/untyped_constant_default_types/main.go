package main

func main() {
	a := 5 / 2
	b := 5 / 2.0
	c := 'w' + 1
	d := 1 < 2
	e := "a" + "b"
	f := 1 + 0i
	g := 1.0 << 3
	h := ^1
	k := 1e6
	var typedA int = a
	var typedB float64 = b
	var typedC int32 = c
	var typedD bool = d
	var typedE string = e
	var typedF complex128 = f
	var typedG int = g
	var typedH int = h
	var typedK float64 = k
	if typedA != 2 || typedB != 2.5 || typedC != 'x' || !typedD || typedE != "ab" || typedF != 1 || typedG != 8 || typedH != -2 || typedK != 1000000 {
		panic("untyped constant defaults changed")
	}
	println("untyped-constant-default-types: ok")
}
