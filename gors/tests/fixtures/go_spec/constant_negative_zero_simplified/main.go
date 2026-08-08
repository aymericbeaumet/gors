package main

func main() {
	const tiny float64 = -1e-1000
	tinyRuntime := tiny
	var neg float64 = -0.0
	z := 0.0
	runtimeNeg := -z

	if tiny != 0 || neg != 0 || runtimeNeg != 0 {
		panic("zero equality changed")
	}
	if 1.0/tinyRuntime < 0 || 1.0/neg < 0 {
		panic("constant conversion produced IEEE negative zero")
	}
	if !(1.0/runtimeNeg < 0) {
		panic("run-time negation of positive zero did not produce negative zero")
	}
}
