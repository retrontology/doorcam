#!/usr/bin/env python3
"""
Simple validation script to check TOML configuration syntax
"""
import sys

try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib
    except ImportError:
        print("Warning: No TOML library available. Install with: pip install tomli")
        sys.exit(0)

def validate_toml_file(filename):
    """Validate TOML file syntax"""
    try:
        with open(filename, 'rb') as f:
            config = tomllib.load(f)
        
        print(f"✓ {filename} is valid TOML")
        
        # Check required sections
        required_sections = ['camera', 'analyzer', 'capture', 'stream', 'display', 'system']
        for section in required_sections:
            if section in config:
                print(f"  ✓ Section [{section}] found")
            else:
                print(f"  ✗ Section [{section}] missing")
        
        return True
        
    except FileNotFoundError:
        print(f"✗ File {filename} not found")
        return False
    except Exception as e:
        print(f"✗ Error parsing {filename}: {e}")
        return False

if __name__ == "__main__":
    filename = "doorcam.toml"
    if len(sys.argv) > 1:
        filename = sys.argv[1]
    
    validate_toml_file(filename)