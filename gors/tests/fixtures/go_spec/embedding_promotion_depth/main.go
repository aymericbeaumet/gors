package main



type Deep struct {
	Label string
	Only  string
}

type Mid struct {
	Deep
	Label string
}

type Top struct {
	Mid
}

type A struct{ N int }
type B struct{ N int }

// Amb has field N at depth 1 through both A and B: selecting amb.N would be
// ambiguous and illegal, but the type itself is legal and the field remains
// reachable through the explicit embedded field names.
type Amb struct {
	A
	B
}

func main() {
	t := Top{Mid: Mid{Deep: Deep{Label: "deep", Only: "only"}, Label: "mid"}}
	if t.Label != "mid" {
		panic("shallowest-depth field must win promotion")
	}
	if t.Mid.Deep.Label != "deep" {
		panic("explicit path must reach the deeper field")
	}
	if t.Only != "only" {
		panic("depth-2 promotion through two embeddings must work")
	}
	t.Label = "updated"
	if t.Mid.Label != "updated" || t.Deep.Label != "deep" {
		panic("promoted assignment must target the shallowest field")
	}
	amb := Amb{A: A{N: 1}, B: B{N: 2}}
	println(t.Label, t.Only, amb.A.N, amb.B.N)
}
