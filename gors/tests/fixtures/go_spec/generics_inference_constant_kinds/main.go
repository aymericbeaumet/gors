package main

func pick[T any](a, b T) T { return a }

func main() {
	var typedInt int = pick(1, 2)
	var typedFloat float64 = pick(1, 2.5)
	var typedRune int32 = pick(1, 'a')
	var typedRuneFloat float64 = pick('a', 2.5)
	var typedComplexInt complex128 = pick(1, 1i)
	var typedComplexFloat complex128 = pick(2.5, 1i)
	var i8 int8 = 5
	var typedInt8 int8 = pick(1, i8)
	var typedUint uint = pick(1, uint(2))
	var firstRune int32 = pick(1, 'a')
	var secondRune int32 = pick('a', 1)
	if typedInt != 1 || typedFloat != 1 || typedRune != 1 || typedRuneFloat != 97 || typedComplexInt != 1 || typedComplexFloat != 2.5 || typedInt8 != 1 || typedUint != 1 || firstRune != 1 || secondRune != 97 {
		panic("generic constant-kind inference changed")
	}
	println("generics-inference-constant-kinds: ok")
}
