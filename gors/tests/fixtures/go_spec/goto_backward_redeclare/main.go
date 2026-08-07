package main

func main() {
	count := 0
	sum := 0
Loop:
	v := count * 10 // re-executed after each backward jump; legal because v is in scope at the goto
	sum += v
	count++
	if count < 3 {
		goto Loop
	}
	if count != 3 || sum != 30 || v != 20 {
		panic("backward goto across a declaration changed")
	}
	println("goto-backward-redeclare: ok")
}
