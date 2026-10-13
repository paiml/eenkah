This are some recommendatinos on data from Storytelling with Data

For a graphite reference, use https://docs.racket-lang.org/graphite/index.html

### ✅ Graphs That Work Well (Almost) All the Time

Knaflic emphasizes that a small, familiar set of visuals can handle most business needs. Here are the go-to chart types from the book:

*   **Simple Text or Big Number**: When your message is just one or two key figures, a simple number with a clear label is often the most powerful visual.
*   **Line Chart**: Your default choice for showing **continuous data**, especially **trends over time**. Lines are excellent for helping an audience see movement and evolution.
*   **Bar Chart (Horizontal and Vertical)**: The best choice for comparing **categorical data**. Horizontal bars are often preferred for long category names as they are easier to read. **Crucially, the y-axis (or x-axis for vertical bars) must always start at a zero baseline** to avoid distorting the comparison.
*   **Slopegraph**: A special type of line chart that is fantastic for comparing two points in time across different groups (e.g., the change in sales for different products from last year to this year).
*   **Scatter Plot**: Use this to show the **relationship between two things** and to identify correlations or outliers.
*   **Waterfall Chart**: A great choice for visualizing how different parts contribute to a total, such as breaking down a starting budget by income and expenses to reach an ending balance.
*   **Table or Heatmap**: When your audience needs to look up specific numbers, a table is best. To help them quickly scan for the most important value, pair it with a heatmap (color coding).
*   **Square Area (Waffle) Chart**: This is the recommended **alternative to a pie chart** for showing parts of a whole. It uses a grid of squares, making it much easier for the human eye to compare proportions accurately.

### ❌ Things to Avoid (Almost) All the Time

Knaflic is famously direct about visual elements that can confuse an audience or misrepresent data. These are the ones to steer clear of:

*   **Pie and Donut Charts**: This is the most famous "don't" from the book. Knaflic argues "pie charts are evil" because humans are terrible at accurately comparing angles and areas. They become especially problematic with more than a few slices. Use a bar chart for comparison or a square area chart for parts of a whole instead.
*   **3D Charts (of any kind)**: 3D effects distort the data, making values look larger or smaller than they are and adding unnecessary clutter that detracts from your message.
*   **Secondary Y-Axes**: Using two different vertical axes on one chart is often misleading and creates false correlations. It is very hard for an audience to accurately interpret.
*   **"Spaghetti" Charts**: Line charts with too many lines (more than 3-5) become unreadable and fail to communicate a clear message. If you have many categories, consider highlighting one or two key lines and graying out the rest, or using a different chart type.

### 📊 Quick Chart Selection Guide

Here is a summary table based on the book's logic to help you choose the right visual for your goal:

| Your Goal | Best Chart Types | Avoid |
| :--- | :--- | :--- |
| **Compare Categories** | Bar Chart, Column Chart | 3D Charts, Pie Charts |
| **Show Trend Over Time** | Line Chart, Slopegraph | Pie Charts, Bar Charts (for continuous time) |
| **Show Parts of a Whole** | Square Area (Waffle) Chart, Stacked Bar | Multiple Pie Charts, 3D Pie Charts |
| **Show Distribution** | Histogram, Box Plot | Overly Complex Plots |
| **Show Relationship** | Scatter Plot | - |
| **Highlight a Number** | Simple Text / Big Number | Any Chart |

### 💡 Key Principles to Remember

Beyond specific chart types, these overarching principles from the book are just as important:

*   **Context is King**: Before you even think about a chart, understand who your audience is, what you need them to know, and what action you want them to take.
*   **Eliminate Clutter**: Every element that doesn't add informative value steals attention from your message. Aggressively remove heavy gridlines, unnecessary labels, borders, and non-essential colors.
*   **Focus Attention Strategically**: Use preattentive attributes like **color, size, and position** to guide your audience's eyes to the most important part of your chart. A single, contrasting color is a powerful tool.
*   **Bars and Lines Do the Heavy Lifting**: Knaflic notes that 7 out of 10 graphs she uses are either bar or line charts. Don't feel pressured to use complex visuals; mastering these two simple, universally understood forms is often the most effective approach.

I hope this summary helps you get back on track with your graph-building. Do you have a specific dataset or message you're trying to visualize right now? I'd be happy to help you apply these principles.
