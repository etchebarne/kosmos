cmake_minimum_required(VERSION 3.20)

project(kosmos_ui VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 REQUIRED COMPONENTS Network Widgets)

add_executable(kosmos-ui
    src/ipc/IpcClient.cpp
    src/ipc/IpcClient.h
    src/main.cpp
)

target_link_libraries(kosmos-ui PRIVATE Qt6::Network Qt6::Widgets)
