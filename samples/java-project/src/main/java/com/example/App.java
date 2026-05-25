package com.example;

import com.example.utils.StringUtils;
import java.util.List;
import java.util.ArrayList;

public class App {
    private final String name;

    public App(String name) {
        this.name = name;
    }

    public String getName() {
        return name;
    }

    public void run() {
        String greeting = StringUtils.capitalize(name);
        System.out.println(greeting);
        List<String> items = new ArrayList<>();
        items.add("hello");
        process(items);
    }

    private void process(List<String> items) {
        for (String item : items) {
            System.out.println(item);
        }
    }

    public static void main(String[] args) {
        App app = new App("world");
        app.run();
    }
}
