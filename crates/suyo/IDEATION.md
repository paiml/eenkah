This project is supposed to be a way to easily graph historical data. Agreggation on several different data sources that can overlap over a period of time is crucial.

For example, imagine CPU usage on a production server. I would like to get easy views on percentages over time at some rate (E.g. per minute) but then it would be useful to see that with an overlay of memory usage over the same period of time. Interesting information can become available when this happens. 

Then, the ability to organize data so that hyper-granular data points like per-second aren't necessary when we want to compare last month to this month, or last month to that same month from the previous year. 

Query-args that work as input for functions and rendering like Graphite is a must. Setting up a single page with no dependencies or being able to have a one-in-all server that works well with minimal configuration sounds ideal.

Maybe something like React.js in the front-end with Rust in the backend so that we can end up with an easy-to-deploy and easy-to-run binary that is able to be the server and do the rendering while the database connection can be done locally with sqlite. 

Some of these are design ideas that are important to me, but not problematic if some of it has to change.

If in doubt about what can be available and implementation refer to https://docs.racket-lang.org/graphite/index.html as a reference
