package main

func f0() {}

func f1() {
	return
}

func f2(a int, b, c string, d ...bool) bool {
	return true
}

func f3(a int, b, c string, d ...bool) (bool, bool) {
	return true, false
}

func f4(a int, b, c string, d ...bool) (e, f bool, g string) {
	return
}
