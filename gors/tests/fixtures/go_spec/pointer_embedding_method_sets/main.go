package main

type Inner struct{ n int }

func (p *Inner) Inc()     { p.n++ }
func (p *Inner) Get() int { return p.n }

type Outer struct {
	*Inner
}

type Counter interface {
	Inc()
	Get() int
}

func makeOuter() Outer { return Outer{Inner: &Inner{n: 100}} }

func main() {
	o := Outer{Inner: &Inner{n: 1}}
	// An embedded *T promotes T and *T methods into both S and *S method sets.
	// The interface copy shares the embedded pointer's pointee identity.
	var c Counter = o
	c.Inc()
	c.Inc()
	if o.Get() != 3 {
		panic("interface copy must share the embedded pointer")
	}

	// Promotion through an embedded pointer does not require the containing
	// value itself to be addressable.
	makeOuter().Inc()
	println(o.Get(), c.Get())
}
