# ROS2 Client

This is a Rust native client library for [ROS2](https://docs.ros.org/en/galactic/index.html). 
It does not link to [rcl](https://github.com/ros2/rcl), 
[rclcpp](https://docs.ros2.org/galactic/api/rclcpp/index.html), or any non-Rust DDS library. 
[RustDDS](https://github.com/jhelovuo/RustDDS) is used for communication.

The API is not identical to `rclcpp` or `rclpy`, because some parts would be very awkward in Rust. For example, there are no callbacks. Rust `async` mechanism is used instead. Alternatively, some of the functionality can be polled using the Metal I/O library.

There is a `.spin()` call, but it is required only to have `ros2-client` execute some background tasks. You can spawn an async task to run it, and retain the flow of control in your code.

Please see the included examples on how to use the various features.

## Features Status

* Topics, Publish and Subscribe ✅
* QoS ✅
* Serialization ✅ - via Serde
* Services: Clients and Servers ✅ (async recommended)
* Actions ✅ (async required)
* Discovery / ROS Graph update events ✅ (async)
* `rosout` logging ✅
* Parameters ✅
    * Parameter Services (remote Parameter manipulation) ✅
* Time support
    * ROS Time ✅
    * Simulated time support ✅
    * Steady time ✅
* Message generation: from `.msg` to `.rs`- experimental
* ROS 2 Security - experimental

## ROS 2 Releases Compatibility

This is what is expected to work. There are no routine tests against older releases.

| ROS 2 Release | `ros2-client` should interoperate? |
| ------------- | :------------ |
| A - E         | Maybe. Not tested. |
| Foxy, Galactic, Humble | Yes. Enable feature `pre-iron-gid` when building `ros2-client` 0.7.5 or newer |
| Iron  | Yes. Not well tested. Requires `ros2-client` 0.7.5 or newer |
| Jazzy | Yes. Requires `ros2-client` 0.7.5 or newer | 


## Version 0.8:
* API change: `ParameterFunc` must now implement `Sync`, so that `Node` is also `Sync`. This helps in using multithreaded async executors.


## New in Version 0.7:
* `NodeName` namespace is no longer allowed to be the empty string, because it confuses ROS 2 tools. Minimum namespace is "/".
* Parameter support, incl. Paramater services
* Time support

### 0.7.1
* Subscribers can `take()` samples with deserialization "seed" value. 
This allows more run-time control of deserialization. Upgrade to RustDDS 0.10.0.

### 0.7.2
* Adapt to separation of CDR encoding from RustDDS.

### 0.7.4
* Implement std `Error` trait for `NameError` and `NodeCreateError`
* Async `wait_for_writer` and `wait_for_reader` results now implement `Send`.

### 0.7.5
* New feature `pre-iron-gid`. The Gid `.msg` definition has changed between ROS2 Humble and Iron. `ros2-client` now uses the newer version by default. Use this feature to revert to the old definition.

## New in Version 0.6:

* Reworked ROS 2 Discovery implementation. Now `Node` has `.status_receiver()`
* Async `.spin()` call to run the Discovery mechanism.
* `Client` has `.wait_for_service()`
* New API for naming Nodes, Topics, Services, Actions, and data types for Topics, Actions, and Services. The new API is more structured to avoid possible confusion and errors from parsing strings.

## New in version 0.5:

* Actions are supported
* async programming interface. This should make a built-in event loop unnecessary, as Rust async executors sort of do that already. This means that `ros2-client` is not going to implement a call similar to  [`rclcpp::spin(..)`](https://docs.ros.org/en/rolling/Concepts/Intermediate/About-Executors.html).

## Example: minimal_action_server and minimal_action_client

These are re-implementations of [similarly named ROS examples](https://docs.ros.org/en/iron/Tutorials/Intermediate/Writing-an-Action-Server-Client/Cpp.html). They should be interoperable with ROS 2 example programs in C++ or Python.

To test this, start a server and then, in a separate terminal, a client, e.g.

`ros2 run examples_rclcpp_minimal_action_server action_server_member_functions`
and
`cargo run --example=minimal_action_client`

or

`cargo run --example=minimal_action_server`
and
`ros2 run examples_rclpy_minimal_action_client client`

You should see the client requesting for a sequence of Fibonacci numbers, and the server providing them until the requested sequence length is reached.

## Example: turtle_teleop

The included example program should be able to communicate with out-of-the-box ROS2 turtlesim example.

Install ROS2 and start the simulator by ` ros2 run turtlesim turtlesim_node`. Then run the `turtle_teleop` example to control the simulator.

![Turtlesim screenshot](examples/turtle_teleop/screenshot.png)

Teleop example program currently has the following keyboard commands:

* Cursor keys: Move turtle
* `q` or `Ctrl-C`: quit
* `r`: reset simulator
* `p`: change pen color (for turtle1 only)
* `a`/`b` : spawn turtle1 / turtle2
* `A`/`B` : kill turtle1 / turtle2
* `1`/`2` : switch control between turtle1 / turtle2
* `d`/`f`/`g`: Trigger or cancel absolute rotation action.

## Example: ros2_service_server

Install ROS2. This has been tested to work against "Galactic" release, using either eProsima FastDDS or RTI Connext DDS (`rmw_connextdds`, not `rmw_connext_cpp`). 

Start server: `cargo run --example=ros2_service_server`

In another terminal or computer, run a client: `ros2 run examples_rclpy_minimal_client client`

## Example: ros2_service_client

Similar to above.

Start server: `ros2 run examples_rclpy_minimal_service service`

Run client: `cargo run --example=ros2_service_client`

## Related Work

* [ros2_rust](https://github.com/ros2-rust/ros2_rust) is closest(?) to an official ROS2 client library. It links to ROS2 `rcl` library written in C.
* [rclrust](https://github.com/rclrust/rclrust) is another ROS2 client library for Rust. It supports also ROS2 Services in addition to Topics. It links to ROS2 libraries, e.g. `rcl` and `rmw`.
* [rus2](https://github.com/marshalshi/rus2) exists, but appears to be inactive since September 2020.

## License

Copyright 2022 Atostek Oy

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

## Acknowledgements

This crate is developed and open-source licensed by [Atostek Oy](https://www.atostek.com/).
