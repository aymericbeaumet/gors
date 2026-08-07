package main

type box struct{ value int }

type talker interface{ Talk() string }

type speaker struct{}

func (speaker) Talk() string { return "talk" }

func nilStructPointerFieldPanics() (recovered interface{}) {
	defer func() { recovered = recover() }()
	var p *box
	_ = p.value
	return nil
}

func nilStructPointerFieldAssignPanics() (recovered interface{}) {
	defer func() { recovered = recover() }()
	var p *box
	p.value = 1
	return nil
}

func nilInterfaceMethodCallPanics() (recovered interface{}) {
	defer func() { recovered = recover() }()
	var value talker
	_ = value.Talk()
	return nil
}

func check(name string, recovered interface{}) {
	if recovered == nil {
		panic(name + ": expected a run-time panic")
	}
	if _, ok := recovered.(error); !ok {
		panic(name + ": panic value does not satisfy error")
	}
}

func main() {
	check("nil pointer field read", nilStructPointerFieldPanics())
	check("nil pointer field assignment", nilStructPointerFieldAssignPanics())
	check("nil interface method call", nilInterfaceMethodCallPanics())
	println("run-time-panics-nil-struct-interface: ok")
}
