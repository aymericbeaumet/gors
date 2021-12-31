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

func main() {
	t.Mv(7)
	T.Mv(t, 7)
	(T).Mv(t, 7)
	f1 := T.Mv
	f1(t, 7)
	f2 := (T).Mv
	f2(t, 7)

	s := []string{}
	f2(s...)
	f2(t, s...)
}
