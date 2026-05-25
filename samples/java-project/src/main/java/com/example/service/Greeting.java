package com.example.service;

public interface Greeting {
    String greet(String name);

    default String greetAll(String[] names) {
        StringBuilder sb = new StringBuilder();
        for (String name : names) {
            sb.append(greet(name)).append("\n");
        }
        return sb.toString();
    }
}
