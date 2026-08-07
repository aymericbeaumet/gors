package main

func pair() (int, string) { return 7, "seven" }

func main() {
	var a, b = pair()
	entries := map[string]int{"x": 1}
	var _, found = entries["x"]
	var _, missing = entries["y"]
	var dyn interface{} = "str"
	var t, ok = dyn.(int)
	var (
		i       int
		u, v, s = 2.0, 3.0, "bar"
	)
	if a != 7 || b != "seven" || !found || missing || t != 0 || ok || i != 0 {
		panic("multi-value var initialization changed")
	}
	if u+v != 5.0 || s != "bar" {
		panic("grouped var defaults changed")
	}
}
