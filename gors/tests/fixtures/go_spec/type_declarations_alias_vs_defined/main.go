package main



type Celsius float64

func (c Celsius) Unit() string { return "C" }

// Alias declaration: Temp and Celsius are identical types, so Temp keeps
// Celsius's method set.
type Temp = Celsius

// Type definition: Kelvinish is a new, distinct type; it does not inherit
// the methods bound to Celsius.
type Kelvinish Celsius

type Uniter interface {
	Unit() string
}

func main() {
	var c Celsius = 21.5
	var t Temp = c // identical types: assignable without conversion
	k := Kelvinish(c)

	println("alias value", float64(t), "unit", t.Unit())

	_, aliasImplements := any(t).(Uniter)
	_, definedImplements := any(k).(Uniter)
	println("alias implements Uniter:", aliasImplements)
	println("defined type implements Uniter:", definedImplements)

	// An alias and its aliased type are interchangeable as dynamic types.
	var boxed any = c
	backViaAlias, ok := boxed.(Temp)
	println("assert Celsius as Temp:", ok, float64(backViaAlias))
	_, okDefined := boxed.(Kelvinish)
	println("assert Celsius as Kelvinish:", okDefined)

	// The defined type keeps the underlying type's operations.
	println("sum", float64(k+k))
}
