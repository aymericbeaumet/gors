package main

func main() {
	var i = 42
	var f = 3.5
	var c = 2i
	var r = 'x'
	var s = "go"
	var b = 1 < 2
	d := 1.0
	k := 'A' + 1
	var typedI int = i
	var typedF float64 = f
	var typedC complex128 = c
	var typedR int32 = r
	var typedS string = s
	var typedB bool = b
	var typedD float64 = d
	var typedK int32 = k
	var viaIface interface{} = f
	if _, ok := viaIface.(float64); !ok {
		panic("untyped float constant default type is not float64")
	}
	if r+1 != 'y' {
		panic("rune arithmetic changed")
	}
	if typedI != 42 || typedF != 3.5 || typedC != 2i || typedR != 'x' || typedS != "go" || !typedB || typedD != 1 || typedK != 'B' {
		panic("variable default types or values changed")
	}
	println("var-decl-default-types: ok")
}
