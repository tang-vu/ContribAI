# Sample Python file for benchmarking
import os
import sys
from django.http import HttpResponse
from django.shortcuts import render

def sample_view(request):
    """A sample view function."""
    return HttpResponse("Hello, World!")

class SampleClass:
    def __init__(self):
        self.value = 42
    
    def method(self):
        return self.value
