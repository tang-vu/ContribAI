// Sample JavaScript file for benchmarking
import React from 'react';
import express from 'express';

const sampleComponent = () => {
    return <div>Hello World</div>;
};

function sampleFunction(req, res) {
    res.json({ message: "Hello" });
}

export { sampleComponent, sampleFunction };
