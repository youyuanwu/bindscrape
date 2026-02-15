#pragma once

// Enum
typedef enum {
    COLOR_RED   = 0,
    COLOR_GREEN = 1,
    COLOR_BLUE  = 2,
} Color;

// Struct with basic fields
typedef struct {
    int x;
    int y;
    unsigned int width;
    unsigned int height;
} Rect;

// Struct with pointer and array
typedef struct {
    const char* name;
    int values[4];
    Color color;
} Widget;

// Function pointer (delegate)
typedef int (*CompareFunc)(const void* a, const void* b);

// Union
typedef union {
    int i;
    float f;
    unsigned char bytes[4];
} Value;

// Struct with anonymous nested union (like in6_addr)
typedef struct {
    union {
        unsigned char  bytes[16];
        unsigned short words[8];
        unsigned int   dwords[4];
    } addr;
    unsigned int scope_id;
} NetAddr;

// Functions
int create_widget(const char* name, Rect bounds, Widget* out);
void destroy_widget(Widget* w);
int widget_count(void);

// #define constants
#define MAX_WIDGETS 256
#define DEFAULT_WIDTH 800
#define DEFAULT_HEIGHT 600
