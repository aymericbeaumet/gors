package main

type my_int = int

type ()

type (
	Polar = polar
)

type (
	Point struct{ x, y float64 }
	polar Point
)

type NewMutex Mutex

type Accounts map[string]string

type testMemoryManager struct {
	description                string
	machineInfo                cadvisorapi.MachineInfo
	assignments                state.ContainerMemoryAssignments
	expectedAssignments        state.ContainerMemoryAssignments
	machineState               state.NUMANodeMap
	expectedMachineState       state.NUMANodeMap
	expectedError              error
	expectedAllocateError      error
	expectedAddContainerError  error
	updateError                error
	removeContainerID          string
	nodeAllocatableReservation v1.ResourceList
	policyName                 policyType
	affinity                   topologymanager.Store
	systemReservedMemory       []kubeletconfig.MemoryReservation
	expectedHints              map[string][]topologymanager.TopologyHint
	expectedReserved           systemReservedMemory
	reserved                   systemReservedMemory
	podAllocate                *v1.Pod
	firstPod                   *v1.Pod
	activePods                 []*v1.Pod
}
